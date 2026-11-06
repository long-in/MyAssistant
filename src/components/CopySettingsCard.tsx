"use client";

import { useState } from "react";
import { ArrowRight, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { CopyRequest, CopyTarget } from "@/lib/copyTargets";
import CopyOptions from "./CopyOptions";
import SpaceChip from "./SpaceChip";

type Props = {
  targets: CopyTarget[];
  /** コピー先の候補。作ったばかりのスペースなど。一覧に無ければ使わない。 */
  suggestedTo: string | null;
  /** 「Copy…」が押された。実行は、最終確認のあとに呼び出し側が行う。 */
  onRequest: (request: CopyRequest) => void;
};

/**
 * 設定のコピー。コピー元とコピー先を選ぶ。コピー先の既定は決めない(上書きされるのは、明示して選んだ先だけ)。
 * 作ったばかりのスペースがあれば、コピー先の候補として最初から選んでおく。
 */
export default function CopySettingsCard({ targets, suggestedTo, onRequest }: Props) {
  const [pickedFrom, setPickedFrom] = useState<string | null>(null);
  const [pickedTo, setPickedTo] = useState<string | null>(null);
  const [includeKeys, setIncludeKeys] = useState(false);
  const [includeProfile, setIncludeProfile] = useState(false);

  // 選択は、いまの一覧に存在するものだけを有効にする(スペースの削除などで、古いIDが残らないように)
  const valid = (id: string | null) => (id !== null && targets.some((t) => t.id === id) ? id : null);
  const to = valid(pickedTo) ?? valid(suggestedTo);
  // コピー元は上書きされないので、未選択ならコピー先以外の先頭(パーソナル)を既定にしてよい
  const from = valid(pickedFrom) !== to ? valid(pickedFrom) : null;
  const fromId = from ?? targets.find((t) => t.id !== to)?.id ?? null;
  const ready = to !== null && fromId !== null && fromId !== to;

  const items = (list: CopyTarget[]) => list.map((t) => ({ value: t.id, label: t.name }));
  const options = (list: CopyTarget[]) =>
    list.map((t) => (
      <SelectItem key={t.id} value={t.id}>
        <SpaceChip name={t.name} color={t.color} />
      </SelectItem>
    ));

  return (
    <Card id="copy-settings">
      <CardHeader>
        <CardTitle>Copy settings</CardTitle>
        <CardDescription>
          Copies the settings of Personal or another space as is. Models, voice, wake words, command permissions and similar settings
          are copied; knowledge, conversations and the pronunciation dictionary are not. The same settings in the destination are
          overwritten, so you confirm before anything is copied.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-2">
          <Select items={items(targets.filter((t) => t.id !== to))} value={fromId} onValueChange={(v) => v && setPickedFrom(v as string)}>
            <SelectTrigger className="min-w-40" aria-label="Copy from">
              <SelectValue placeholder="Copy from" />
            </SelectTrigger>
            <SelectContent>{options(targets.filter((t) => t.id !== to))}</SelectContent>
          </Select>
          <ArrowRight className="size-4 text-muted-foreground" />
          <Select items={items(targets)} value={to} onValueChange={(v) => v && setPickedTo(v as string)}>
            <SelectTrigger className="min-w-40" aria-label="Copy to">
              <SelectValue placeholder="Copy to (choose)" />
            </SelectTrigger>
            <SelectContent>{options(targets)}</SelectContent>
          </Select>
        </div>
        <CopyOptions includeKeys={includeKeys} includeProfile={includeProfile} onKeys={setIncludeKeys} onProfile={setIncludeProfile} />
        <div className="flex items-center gap-3">
          <Button disabled={!ready} onClick={() => ready && onRequest({ from: fromId, to, includeKeys, includeProfile })}>
            <Copy />
            Copy…
          </Button>
          {to === null && <span className="text-xs text-muted-foreground">Choose where to copy to.</span>}
        </div>
      </CardContent>
    </Card>
  );
}
