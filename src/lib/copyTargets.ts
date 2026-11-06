import { PERSONAL_ID, type Space } from "@/lib/api";
import { tr } from "@/lib/i18n";

/** 設定のコピー元・コピー先に選べるワークスペース(パーソナルと各スペース)。 */
export type CopyTarget = { id: string; name: string; color: string | null };

/** 設定のコピーの依頼。`from`の設定を`to`へ写す。`to`の同じ項目は上書きされる。 */
export type CopyRequest = {
  from: string;
  to: string;
  /** APIキー・AWS設定もコピーする。 */
  includeKeys: boolean;
  /** パーソナルデータもコピーする。 */
  includeProfile: boolean;
};

export const copyTargets = (spaces: Space[]): CopyTarget[] => [
  { id: PERSONAL_ID, name: tr("パーソナル"), color: null },
  ...spaces.map((s) => ({ id: s.id, name: s.name, color: s.color })),
];
