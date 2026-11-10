"use client";

import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { FileText, RefreshCw } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  type SandboxFiles,
  type SandboxStatus,
  getSandboxEnabled,
  getSandboxFiles,
  getSandboxStatus,
  reportError,
  resetSandbox,
  revealSandboxFile,
} from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Heading, StatusPill } from "@/components/GlobalPanels";
import SandboxDockerfile from "@/components/SandboxDockerfile";

/**
 * エージェントごとのサンドボックス(専用のDockerコンテナでコマンドを実行する)の状態と、Dockerfile・環境変数の確認。
 * 有効/無効は全体設定(`SandboxSwitch`)で切り替えるので、ここにスイッチは無い。
 * Dockerfileと環境変数(sandbox.env)はワークスペースのファイルで、ここでは状態の確認とFinderでの表示だけを行う。
 * 環境変数の値は画面へ渡されない(名前だけ表示する)。
 */
export default function SandboxPanel({ workspaceId }: { workspaceId: string }) {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [status, setStatus] = useState<SandboxStatus | null>(null);
  const [files, setFiles] = useState<SandboxFiles | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [s, f] = await Promise.all([getSandboxStatus(), getSandboxFiles()]);
      setStatus(s);
      setFiles(f);
    } catch (e) {
      toast.error(reportError(e));
    }
  }, []);

  useEffect(() => {
    getSandboxEnabled()
      .then(setEnabled)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  // 有効になったときに状態を読む
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    Promise.all([getSandboxStatus(), getSandboxFiles()])
      .then(([s, f]) => {
        if (cancelled) return;
        setStatus(s);
        setFiles(f);
      })
      .catch((e) => toast.error(reportError(e)));
    return () => {
      cancelled = true;
    };
  }, [enabled]);

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
            <p>{t("このエージェントのコマンドを、専用のDockerコンテナの中で実行します。")}</p>
            <ul>
              <li>{t("コンテナから見えるのは、このエージェントのナレッジ(/workspace)と専用ボリュームだけです。ほかのエージェントや ~/.aws には届きません。")}</li>
              <li>{t("Dockerが使えないときは、ホストでは実行せずエラーにします。")}</li>
              <li>{t("コマンドはLinux(Debian)上で動きます。brew や open などmacOS向けのコマンドは使えません。")}</li>
              <li>{t("sandbox.env はこのエージェントのフォルダにあり、編集できます。")}</li>
            </ul>
          </Heading>
          <p className="text-xs text-muted-foreground">
            {enabled
              ? t("有効です。このエージェントのコマンドは専用のDockerコンテナで実行されます。切り替えは全体設定で行います。")
              : t("無効です。コマンドはホストで実行されます。切り替えは全体設定で行います。")}
          </p>
        </div>
        <StatusPill on={!!enabled}>{enabled ? t("オン") : t("オフ")}</StatusPill>
      </div>

      {enabled && (
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

          <SandboxDockerfile key={workspaceId} agent={{ id: workspaceId }} embedded />

          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="hud-label">sandbox.env</span>
              <Button size="sm" variant="outline" onClick={() => run("reveal-env", () => revealSandboxFile("env"))}>
                <FileText className="size-3.5" />
                {t("ファイルを表示")}
              </Button>
              <Button size="sm" variant="outline" onClick={() => void refresh()}>
                {t("再読み込み")}
              </Button>
            </div>
            {files?.env.error ? (
              <p className="text-xs text-destructive">
                {files.env.error.line != null
                  ? t("{line}行目が正しくありません: {message}", { line: files.env.error.line, message: files.env.error.message })
                  : files.env.error.message}
              </p>
            ) : files && files.env.names.length > 0 ? (
              <div className="flex flex-wrap gap-1.5">
                {files.env.names.map((n) => (
                  <Badge key={n} variant="secondary" className="font-mono">
                    {n}
                  </Badge>
                ))}
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">{t("環境変数は未設定です。")}</p>
            )}
            <p className="text-xs text-muted-foreground">
              {t("NAME=value の形式で書きます。値はここには表示されません。保存すると次のコマンドから反映されます。")}
            </p>
          </div>
        </div>
      )}
    </section>
  );
}
