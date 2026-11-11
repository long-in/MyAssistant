"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { FolderOpen } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  getSandboxDataDir,
  openSandboxDataDir,
  reportError,
} from "@/lib/api";
import { Button } from "@/components/ui/button";

/**
 * エージェントごとのサンドボックスの設定。
 * - 担当システムのフォルダ(ナレッジ直下の`data/`。ソースコードなどを置く)。コンテナでは`/knowledge/data`として読み書きできる。
 * コンテナ内の Claude Code のモデル・許可ツールは settings.json(`ClaudeSettingsEditor`)で、1回の指示の上限時間もそこで扱う。
 */
export default function SandboxAgentSettings() {
  const t = useT();
  const [dir, setDir] = useState<string | null>(null);

  useEffect(() => {
    getSandboxDataDir().then(setDir).catch((e) => toast.error(reportError(e)));
  }, []);

  const openDir = async () => {
    try {
      await openSandboxDataDir();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <span className="hud-label">/knowledge/data</span>
        <div className="flex flex-wrap items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-mono text-xs" title={dir ?? undefined}>{dir ?? "…"}</span>
          <Button size="sm" variant="outline" onClick={() => void openDir()}>
            <FolderOpen className="size-3.5" />
            {t("Finder で開く")}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("ナレッジの中のこのフォルダが、コンテナの /knowledge/data として読み書きできます。担当システムのソースコードなどは、ここに置くか git clone してください。コンテナ内の作業でファイルが変更・削除されることがあります。")}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("ナレッジ画面・検索・整理には出ず、バックアップにも含まれません。エージェントを削除するときは、ナレッジと一緒に残すか削除するかが決まります。")}
        </p>
      </div>
    </div>
  );
}
