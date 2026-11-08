"use client";

import { useState } from "react";
import { ArrowRight, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Heading } from "@/components/GlobalPanels";
import { useT } from "@/lib/i18n";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { CopyRequest, CopyTarget } from "@/lib/copyTargets";
import CopyOptions from "./CopyOptions";
import AgentChip from "./AgentChip";

type Props = {
  targets: CopyTarget[];
  /** コピー先の候補。作ったばかりのエージェントなど。一覧に無ければ使わない。 */
  suggestedTo: string | null;
  /** コピー先を固定する(エージェントの設定画面から、そのエージェントへ写すとき)。選択欄は出さない。 */
  fixedTo?: string;
  /** 「Copy…」が押された。実行は、最終確認のあとに呼び出し側が行う。 */
  onRequest: (request: CopyRequest) => void;
};

/**
 * 設定のコピー。コピー元とコピー先を選ぶ。コピー先の既定は決めない(上書きされるのは、明示して選んだ先だけ)。
 * 作ったばかりのエージェントがあれば、コピー先の候補として最初から選んでおく。
 */
export default function CopySettingsCard({ targets, suggestedTo, fixedTo, onRequest }: Props) {
  const t = useT();
  const [pickedFrom, setPickedFrom] = useState<string | null>(null);
  const [pickedTo, setPickedTo] = useState<string | null>(null);
  const [includeKeys, setIncludeKeys] = useState(false);
  const [includeDictionary, setIncludeDictionary] = useState(false);

  // 選択は、いまの一覧に存在するものだけを有効にする(エージェントの削除などで、古いIDが残らないように)
  const valid = (id: string | null) => (id !== null && targets.some((t) => t.id === id) ? id : null);
  const to = (fixedTo !== undefined ? valid(fixedTo) : valid(pickedTo)) ?? valid(suggestedTo);
  // コピー元は上書きされないので、未選択ならコピー先以外の先頭(パーソナル)を既定にしてよい
  const from = valid(pickedFrom) !== to ? valid(pickedFrom) : null;
  const fromId = from ?? targets.find((t) => t.id !== to)?.id ?? null;
  const ready = to !== null && fromId !== null && fromId !== to;

  const items = (list: CopyTarget[]) => list.map((t) => ({ value: t.id, label: t.name }));
  const options = (list: CopyTarget[]) =>
    list.map((t) => (
      <SelectItem key={t.id} value={t.id}>
        <AgentChip name={t.name} color={t.color} />
      </SelectItem>
    ));

  return (
    <section id="copy-settings" className="hud-panel hud-accent scroll-mt-6 px-5 py-4">
      <Heading title="Copy Settings" help={t("設定のコピー")}>
        <p>
          {t("パーソナルまたはほかのエージェントの設定を、そのまま写します。モデル・音声・起動ワード・コマンド権限などが対象で、ナレッジ・会話は写しません。APIキー・読み上げ辞書は、選んだときだけ写します。パーソナルデータとエージェントデータは写しません。コピー先の同じ設定は上書きされるため、コピーの前に確認します。")}
        </p>
      </Heading>
      <div className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-2">
          <Select items={items(targets.filter((t) => t.id !== to))} value={fromId} onValueChange={(v) => v && setPickedFrom(v as string)}>
            <SelectTrigger className="min-w-40" aria-label={t("コピー元")}>
              <SelectValue placeholder={t("コピー元")} />
            </SelectTrigger>
            <SelectContent>{options(targets.filter((t) => t.id !== to))}</SelectContent>
          </Select>
          <ArrowRight className="size-4 text-muted-foreground" />
          {fixedTo !== undefined && to !== null ? (
            <span className="flex min-h-8 min-w-40 items-center rounded-lg border px-3 text-sm" aria-label={t("コピー先")}>
              <AgentChip name={targets.find((x) => x.id === to)?.name ?? ""} color={targets.find((x) => x.id === to)?.color ?? null} />
            </span>
          ) : (
            <Select items={items(targets)} value={to} onValueChange={(v) => v && setPickedTo(v as string)}>
              <SelectTrigger className="min-w-40" aria-label={t("コピー先")}>
                <SelectValue placeholder={t("コピー先を選択")} />
              </SelectTrigger>
              <SelectContent>{options(targets)}</SelectContent>
            </Select>
          )}
        </div>
        <CopyOptions includeKeys={includeKeys} includeDictionary={includeDictionary} onKeys={setIncludeKeys} onDictionary={setIncludeDictionary} />
        <div className="flex items-center gap-3">
          <Button disabled={!ready} onClick={() => ready && onRequest({ from: fromId, to, includeKeys, includeDictionary })}>
            <Copy />
            {t("コピー…")}
          </Button>
          {to === null && <span className="text-xs text-muted-foreground">{t("コピー先を選んでください。")}</span>}
        </div>
      </div>
    </section>
  );
}
