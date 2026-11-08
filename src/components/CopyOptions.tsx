"use client";

import { useT } from "@/lib/i18n";
import { Switch } from "@/components/ui/switch";

type Props = {
  includeKeys: boolean;
  includeDictionary: boolean;
  onKeys: (v: boolean) => void;
  onDictionary: (v: boolean) => void;
};

/** 設定のコピーで、既定ではコピーしないもの(APIキー・読み上げ辞書)を、選んだときだけ含めるスイッチ。 */
export default function CopyOptions({ includeKeys, includeDictionary, onKeys, onDictionary }: Props) {
  const t = useT();
  return (
    <>
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>
          {t("APIキーもコピーする")}
          <span className="block text-xs text-muted-foreground">
            {t("Anthropic・AWS・Google・Aquaのキーを、コピー先にも保存します(コピー先の同じキーは上書き)。")}
          </span>
        </span>
        <Switch checked={includeKeys} onCheckedChange={onKeys} />
      </label>
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>
          {t("読み上げ辞書もコピーする")}
          <span className="block text-xs text-muted-foreground">
            {t("登録した読みを、コピー先にも写します(コピー先の辞書は置き換わります)。")}
          </span>
        </span>
        <Switch checked={includeDictionary} onCheckedChange={onDictionary} />
      </label>
    </>
  );
}
