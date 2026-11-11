"use client";

import { Check, Lightbulb, Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PaneHeader } from "@/components/GlobalPanels";
import { useT } from "@/lib/i18n";
import type { Proposal, ProposalKind } from "@/lib/api";

/** 種類ごとの見出し・説明・承認ボタンの文言(画面に並べる順)。 */
const SECTIONS: { kind: ProposalKind; title: string; description: string; accept: string }[] = [
  { kind: "memory", title: "記憶の候補", description: "会話での指摘や好みです。承認すると「覚えておくこと」に加わり、毎回の会話で踏まえます。", accept: "記憶に追加" },
  { kind: "skill", title: "スキルの下書き", description: "繰り返し使えそうな手順です。承認するとスキルとして登録されます。", accept: "スキルを作成" },
  { kind: "merge", title: "ノートの統合", description: "内容が似ているノートです。承認すると、AIが一方の内容をもう一方へ取り込み、取り込んだノートを削除します(削除前の版は残ります)。", accept: "統合する" },
  { kind: "review", title: "見直し", description: "長く更新されていない、またはカテゴリが未設定のノートです。内容を確認したら「確認した」を押してください。", accept: "確認した" },
];

/** 学びの提案(整理・棚卸しが出したもの)の一覧。承認したものだけ反映する。 */
export default function ProposalsPanel({
  proposals,
  busyId,
  onDecide,
  onOpenNote,
}: {
  proposals: Proposal[];
  /** 処理中の提案のID。 */
  busyId: string | null;
  onDecide: (p: Proposal, accept: boolean) => void;
  onOpenNote: (path: string) => void;
}) {
  const t = useT();
  const noteLink = (path: string) => (
    <button type="button" className="break-all text-primary underline-offset-2 hover:underline" onClick={() => onOpenNote(path)}>
      {path}
    </button>
  );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PaneHeader icon={Lightbulb} title="Proposals" />
      <div className="flex-1 overflow-y-auto px-10 py-6">
        <div className="mx-auto flex max-w-3xl flex-col gap-8">
          {proposals.length === 0 && (
            <p className="text-sm text-muted-foreground">
              {t("提案はありません。「今すぐ整理」で会話から学びを、「棚卸し」でノートの統合・見直しの候補を探します。")}
            </p>
          )}
          {SECTIONS.map((s) => {
            const list = proposals.filter((p) => p.kind === s.kind);
            if (list.length === 0) return null;
            return (
              <section key={s.kind}>
                <h2 className="mb-1 text-sm font-semibold">
                  {t(s.title)}
                  <span className="ml-2 text-xs font-normal text-muted-foreground">{t("{n}件", { n: list.length })}</span>
                </h2>
                <p className="mb-3 text-xs text-muted-foreground">{t(s.description)}</p>
                <ul className="flex flex-col divide-y rounded-md border">
                  {list.map((p) => {
                    const busy = busyId === p.id;
                    return (
                      <li key={p.id} className="flex items-start gap-2 px-3 py-2 text-sm">
                        <div className="min-w-0 flex-1 break-words">
                          {p.kind === "skill" && p.payload ? (
                            <>
                              <p className="font-mono text-xs text-muted-foreground">/{p.payload.name}</p>
                              <p>{p.payload.description}</p>
                              <details className="mt-1">
                                <summary className="cursor-pointer text-xs text-muted-foreground">{t("手順を見る")}</summary>
                                <pre className="mt-1 whitespace-pre-wrap rounded bg-muted/40 p-2 text-xs">{p.payload.body}</pre>
                              </details>
                            </>
                          ) : p.kind === "merge" && p.payload ? (
                            <>
                              <p>{t("「{drop}」を「{keep}」へ統合する", { drop: p.payload.drop, keep: p.payload.keep })}</p>
                              <p className="mt-1 flex flex-wrap gap-x-3 text-xs">
                                {noteLink(p.payload.keep)}
                                {noteLink(p.payload.drop)}
                              </p>
                            </>
                          ) : p.kind === "review" && p.payload ? (
                            <>
                              <p className="text-xs">{noteLink(p.payload.path)}</p>
                              <p>{t(p.text)}</p>
                            </>
                          ) : (
                            <p>{p.text}</p>
                          )}
                          <p className="mt-1 text-xs text-muted-foreground">{p.created_at.slice(0, 10)}</p>
                        </div>
                        <div className="flex shrink-0 gap-1">
                          <Button size="sm" variant="outline" disabled={busyId !== null} onClick={() => onDecide(p, true)}>
                            {busy ? <Loader2 className="animate-spin" /> : <Check />} {t(s.accept)}
                          </Button>
                          <Button size="sm" variant="ghost" disabled={busyId !== null} title={t("却下(同じ提案は二度と出ません)")} onClick={() => onDecide(p, false)}>
                            <X /> {t("却下")}
                          </Button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </section>
            );
          })}
        </div>
      </div>
    </div>
  );
}
