"use client";

import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { RefreshCw } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  type SandboxStatus,
  getSandboxEnabled,
  getSandboxStatus,
  reportError,
  resetSandbox,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Heading, StatusPill } from "@/components/GlobalPanels";
import SandboxDockerfile from "@/components/SandboxDockerfile";
import SandboxAgentSettings from "@/components/SandboxAgentSettings";
import SandboxEnvEditor from "@/components/SandboxEnvEditor";

/**
 * エージェントごとのサンドボックス(専用のDockerコンテナの中で Claude Code が作業する)の状態と設定、Dockerfile・環境変数の確認。
 * 有効/無効は全体設定(`SandboxSwitch`)で切り替えるので、ここにスイッチは無い。
 * Dockerfileと環境変数(sandbox.env)はワークスペースのファイルで、ここでは状態の確認とFinderでの表示だけを行う。
 */
export default function SandboxPanel({ workspaceId }: { workspaceId: string }) {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [status, setStatus] = useState<SandboxStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getSandboxStatus());
    } catch (e) {
      toast.error(reportError(e));
    }
  }, []);

  useEffect(() => {
    getSandboxEnabled()
      .then(setEnabled)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  // 全体設定で無効でも、有効にする前に準備できるよう状態を読む
  useEffect(() => {
    let cancelled = false;
    getSandboxStatus()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch((e) => toast.error(reportError(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  const run = async (key: string, fn: () => Promise<void>, ok?: string) => {
    setBusy(key);
    try {
      await fn();
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const dockerLabel =
    status?.docker === "ready" ? t("Docker: 使えます") : status?.docker === "stopped" ? t("Docker: 停止中") : t("Docker: 未導入");

  return (
    <section className="hud-panel hud-accent flex flex-col gap-4 px-5 py-4">
      <div className="flex items-center justify-between gap-4">
        <div>
          <Heading title="Sandbox" help={t("サンドボックス")}>
            <p>{t("このエージェントへの指示を、専用のDockerコンテナの中の Claude Code が受け持ちます。調査・編集・コマンドの実行はすべてコンテナの中で行います。")}</p>
            <ul>
              <li>{t("コンテナから見えるのは、担当フォルダ(/workspace)、ナレッジ(/knowledge)、専用ボリュームだけです。ほかのエージェントや ~/.aws には届きません。")}</li>
              <li>{t("認証情報(sandbox.env と AWS の一時キー)は、指示ごとに環境変数で渡します。")}</li>
              <li>{t("Dockerが使えないときは、ホストでは実行せずエラーにします。")}</li>
              <li>{t("コマンドはLinux(Debian)上で動きます。brew や open などmacOS向けのコマンドは使えません。")}</li>
              <li>{t("sandbox.env(コンテナへ渡す環境変数)は、この画面で編集できます。")}</li>
            </ul>
          </Heading>
          <p className="text-xs text-muted-foreground">
            {enabled
              ? t("有効です。このエージェントへの指示は、専用のDockerコンテナの中の Claude Code が受け持ちます。切り替えは全体設定で行います。")
              : t("全体設定で無効になっています。今は、指示をこのアプリが直接処理し、コマンドはホストで実行されます。有効にする前に、ここで準備できます。")}
          </p>
        </div>
        <StatusPill on={!!enabled}>{enabled ? t("オン") : t("オフ")}</StatusPill>
      </div>

      <div className="flex flex-col gap-4 border-t pt-4">
        <div className="flex flex-wrap items-center gap-2">
          <StatusPill on={status?.docker === "ready"}>{dockerLabel}</StatusPill>
          <StatusPill on={!!status?.container_running}>{status?.container_running ? t("コンテナ: 実行中") : t("コンテナ: 停止")}</StatusPill>
          <Button size="icon-sm" variant="ghost" onClick={() => void refresh()} aria-label={t("状態を更新")}>
            <RefreshCw className="size-3.5" />
          </Button>
        </div>
        {status?.container_name && (
          <p className="text-xs text-muted-foreground">
            {t("コンテナ名")}: <span className="font-mono">{status.container_name}</span>
          </p>
        )}
        {status?.docker === "missing" && (
          <p className="text-xs text-muted-foreground">{t("Dockerが見つかりません。Docker Desktop などをインストールして起動してください。")}</p>
        )}
        {status?.docker === "stopped" && (
          <p className="text-xs text-muted-foreground">{t("Dockerに接続できません。Docker Desktop などを起動してください。")}</p>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={status?.docker !== "ready" || !!busy}
            onClick={() => run("reset", resetSandbox, t("コンテナを作り直します(次のコマンドで作成されます)。"))}
          >
            {t("コンテナを作り直す")}
          </Button>
        </div>

        <div className="border-t pt-4">
          <SandboxAgentSettings key={workspaceId} />
        </div>

        <SandboxDockerfile key={workspaceId} agent={{ id: workspaceId }} embedded />

        <div className="border-t pt-4">
          <SandboxEnvEditor key={workspaceId} />
        </div>
      </div>
    </section>
  );
}
