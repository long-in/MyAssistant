"use client";

import { Switch } from "@/components/ui/switch";

type Props = {
  includeKeys: boolean;
  includeProfile: boolean;
  onKeys: (v: boolean) => void;
  onProfile: (v: boolean) => void;
};

/** 設定のコピーで、既定ではコピーしないもの(APIキー・パーソナルデータ)を、選んだときだけ含めるスイッチ。 */
export default function CopyOptions({ includeKeys, includeProfile, onKeys, onProfile }: Props) {
  return (
    <>
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>
          Also copy API keys
          <span className="block text-xs text-muted-foreground">
            Saves the Anthropic, AWS, Google and Aqua keys in the destination too (the same keys there are overwritten).
          </span>
        </span>
        <Switch checked={includeKeys} onCheckedChange={onKeys} />
      </label>
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>
          Also copy personal data
          <span className="block text-xs text-muted-foreground">
            Name, how to address you, birthday and so on. You normally should not copy these to a work space.
          </span>
        </span>
        <Switch checked={includeProfile} onCheckedChange={onProfile} />
      </label>
    </>
  );
}
