"use client";

import { useT } from "@/lib/i18n";
import { Switch } from "@/components/ui/switch";

type Props = {
  includeKeys: boolean;
  includeProfile: boolean;
  onKeys: (v: boolean) => void;
  onProfile: (v: boolean) => void;
};

/** 設定のコピーで、既定ではコピーしないもの(APIキー・パーソナルデータ)を、選んだときだけ含めるスイッチ。 */
export default function CopyOptions({ includeKeys, includeProfile, onKeys, onProfile }: Props) {
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
          {t("パーソナルデータもコピーする")}
          <span className="block text-xs text-muted-foreground">
            {t("名前・呼ばれ方・誕生日など。仕事用のスペースへは、通常コピーしないでください。")}
          </span>
        </span>
        <Switch checked={includeProfile} onCheckedChange={onProfile} />
      </label>
    </>
  );
}
