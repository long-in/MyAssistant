"use client";

import { useT } from "@/lib/i18n";
import {
  PROVIDER_LABELS,
  type KeyStatus,
  type ModelInfo,
  type Provider,
  type Route,
} from "@/lib/api";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

type Props = {
  route: Route | null;
  models: ModelInfo[];
  keyStatus: Partial<KeyStatus>;
  onChange: (route: Route) => void;
};

/** プロバイダーとモデルの手動選択。左メニューの下部に縦に並べる。 */
export default function ModelSelector({ route, models, keyStatus, onChange }: Props) {
  const t = useT();
  const usable = models.filter((m) => keyStatus[m.provider]);
  const providers = [...new Set(usable.map((m) => m.provider))];
  const current = route;
  const providerItems = providers.map((p) => ({ value: p, label: PROVIDER_LABELS[p] }));
  const modelItems = usable
    .filter((m) => m.provider === current?.provider)
    .map((m) => ({ value: m.id, label: m.label }));

  return (
    <div className="flex flex-col gap-2">
      {current && (
        <>
          <Select
            items={providerItems}
            value={current.provider}
            onValueChange={(v) => {
              const provider = v as Provider;
              const m = usable.find((x) => x.provider === provider);
              if (m) onChange({ provider, model: m.id });
            }}
          >
            <SelectTrigger className="w-full" aria-label={t("プロバイダー")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {providerItems.map((i) => (
                <SelectItem key={i.value} value={i.value}>{i.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select
            items={modelItems}
            value={current.model}
            onValueChange={(v) =>
              v && onChange({ provider: current.provider, model: v as string })
            }
          >
            <SelectTrigger className="w-full" aria-label={t("モデル")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {modelItems.map((i) => (
                <SelectItem key={i.value} value={i.value}>{i.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </>
      )}
    </div>
  );
}
