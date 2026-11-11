"use client";

import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useT } from "@/lib/i18n";
import {
  LEADER_ID,
  PERSONAL_ID,
  type SandboxStatus,
  getSandboxStatus,
  reportError,
} from "@/lib/api";
import { Heading } from "@/components/GlobalPanels";
import SandboxSteps from "@/components/SandboxSteps";
import SandboxDockerfile from "@/components/SandboxDockerfile";
import SandboxAgentSettings from "@/components/SandboxAgentSettings";
import SandboxEnvEditor from "@/components/SandboxEnvEditor";
import ClaudeSettingsEditor from "@/components/ClaudeSettingsEditor";
import SandboxTimeout from "@/components/SandboxTimeout";

/** イメージを再ビルドしたあと、まだコンテナを作り直していないエージェントのID(画面を離れても覚えておく)。 */
const staleContainers = new Set<string>();

/**
 * エージェントごとのサンドボックス(専用のDockerコンテナの中で Claude Code が作業する)の準備(イメージのビルド→コンテナの起動)と設定、Dockerfile・環境変数の確認。
 * ワークのエージェントは常にサンドボックスで動くので、切り替えは無い(Claude Code の設定は下の settings.json と上限時間)。
 * Dockerfileと環境変数(sandbox.env)はワークスペースのファイルで、ここでは状態の確認とFinderでの表示だけを行う。
 */
export default function SandboxPanel({ workspaceId }: { workspaceId: string }) {
  const t = useT();
  const [status, setStatus] = useState<SandboxStatus | null>(null);
  const [dockerfileDirty, setDockerfileDirty] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [containerStale, setContainerStaleState] = useState(() => staleContainers.has(workspaceId));
  const setContainerStale = useCallback(
    (v: boolean) => {
      if (v) staleContainers.add(workspaceId);
      else staleContainers.delete(workspaceId);
      setContainerStaleState(v);
    },
    [workspaceId],
  );

  const refresh = useCallback(async () => {
    try {
      setStatus(await getSandboxStatus());
    } catch (e) {
      toast.error(reportError(e));
    }
  }, []);

  // 画面を開いたときに状態を読む
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

  // 共通の Dockerfile を別の画面で変えて戻ってきたときなど、画面に戻ったら状態を読み直す
  useEffect(() => {
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const editDockerfile = () => {
    setDetailsOpen(true);
    setTimeout(() => document.getElementById("sandbox-dockerfile")?.scrollIntoView({ behavior: "smooth", block: "start" }), 0);
  };

  return (
    <section className="hud-panel hud-accent flex flex-col gap-4 px-5 py-4">
      <div className="flex items-center justify-between gap-4">
        <div>
          <Heading title="Sandbox" help={t("サンドボックス")}>
            <p>{t("このエージェントへの指示を、専用のDockerコンテナの中の Claude Code が受け持ちます。調査・編集・コマンドの実行はすべてコンテナの中で行います。")}</p>
            <ul>
              <li>{t("コンテナから見えるのは、ナレッジ(/knowledge。担当フォルダ /knowledge/data を含む)と専用ボリュームだけです。ほかのエージェントや ~/.aws には届きません。")}</li>
              <li>{t("認証情報(AI Link に登録した AWS の一時キー・sandbox.env)は、指示ごとに環境変数で渡します。")}</li>
              <li>{t("Dockerが使えないときは、ホストでは実行せずエラーにします。")}</li>
              <li>{t("チャットは、イメージのビルドとコンテナの起動を済ませてから使えます。")}</li>
              <li>{t("コマンドはLinux(Debian)上で動きます。brew や open などmacOS向けのコマンドは使えません。")}</li>
              <li>{t("sandbox.env(コンテナへ渡す環境変数)は、この画面で編集できます。")}</li>
            </ul>
          </Heading>
          <p className="text-xs text-muted-foreground">
            {workspaceId === PERSONAL_ID || workspaceId === LEADER_ID
              ? t("このエージェントは、モデルに「コンテナ内の Claude Code」を選んだときだけ、このコンテナで動きます(Bedrock を使うときはこれを選びます)。Claude Code のモデルや許可するツールは、下の settings.json で設定します。")
              : t("ワークのエージェントは、常にこのコンテナで動きます(切り替えはありません)。Claude Code のモデルや許可するツールは、下の settings.json で設定します。")}
          </p>
        </div>
      </div>

      <div className="flex flex-col gap-4 border-t pt-4">
        <SandboxSteps
          workspaceId={workspaceId}
          status={status}
          onChange={refresh}
          dockerfileDirty={dockerfileDirty}
          containerStale={containerStale}
          onRebuilt={() => setContainerStale(true)}
          onContainerRefreshed={() => setContainerStale(false)}
          onEditDockerfile={editDockerfile}
        />

        <details open={detailsOpen} onToggle={(e) => setDetailsOpen(e.currentTarget.open)} className="group border-t pt-4">
          <summary className="hud-label cursor-pointer select-none">{t("詳細設定(任意)")}</summary>
          <p className="mt-2 text-xs text-muted-foreground">
            {t("Dockerfile を変更したときは、上のチェックリストで ② イメージのビルド → ③ コンテナの起動 をやり直してください。環境変数・settings.json・上限時間の変更は、次の指示から有効になります(やり直しは不要です)。")}
          </p>
          <div className="mt-4 flex flex-col gap-4">
            <SandboxAgentSettings key={workspaceId} />

            <div id="sandbox-dockerfile" className="scroll-mt-4">
              <SandboxDockerfile key={workspaceId} agent={{ id: workspaceId }} embedded onChange={refresh} onDirtyChange={setDockerfileDirty} />
            </div>

            <div className="border-t pt-4">
              <SandboxEnvEditor key={workspaceId} />
            </div>

            <div className="border-t pt-4">
              <SandboxTimeout key={workspaceId} />
            </div>

            <div className="border-t pt-4">
              <ClaudeSettingsEditor key={workspaceId} />
            </div>
          </div>
        </details>
      </div>
    </section>
  );
}
