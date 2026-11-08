"use client";

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { FileText, Loader2, RefreshCw } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  SANDBOX_BUILD_EVENT,
  type SandboxBuildProgress,
  type SandboxFiles,
  type SandboxStatus,
  buildSandboxImage,
  getSandboxEnabled,
  getSandboxFiles,
  getSandboxStatus,
  reportError,
  resetSandbox,
  resetSandboxDockerfile,
  revealSandboxFile,
  setSandboxEnabled,
} from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Heading, StatusPill } from "@/components/GlobalPanels";

/**
 * エージェントごとのサンドボックス(専用のDockerコンテナでコマンドを実行する)の設定。
 * Dockerfileと環境変数(sandbox.env)はワークスペースのファイルで、ここでは状態の確認とFinderでの表示だけを行う。
 * 環境変数の値は画面へ渡されない(名前だけ表示する)。
 */
export default function SandboxPanel({ workspaceId }: { workspaceId: string }) {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [status, setStatus] = useState<SandboxStatus | null>(null);
  const [files, setFiles] = useState<SandboxFiles | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState("");

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

  // ビルドの進捗(このエージェントの分だけ)
  useEffect(() => {
    const un = listen<SandboxBuildProgress>(SANDBOX_BUILD_EVENT, (e) => {
      if (e.payload.workspace_id === workspaceId) setLog(e.payload.line);
    });
    return () => {
      un.then((f) => f());
    };
  }, [workspaceId]);

  const change = async (next: boolean) => {
    setEnabled(next);
    try {
      await setSandboxEnabled(next);
      if (next) await refresh();
    } catch (e) {
      setEnabled(!next);
      toast.error(reportError(e));
    }
  };

  const run = async (key: string, fn: () => Promise<void>, ok?: string) => {
    setBusy(key);
    try {
      await fn();
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(null);
      setLog("");
      await refresh();
    }
  };

  const confirmReset = async () => {
    const yes = await ask(t("Dockerfile を標準の内容で上書きします。編集した内容は失われます。続行しますか?"), {
      title: t("Dockerfile を標準に戻す"),
      kind: "warning",
      okLabel: t("上書きする"),
      cancelLabel: t("キャンセル"),
    });
    if (yes) await run("reset-dockerfile", resetSandboxDockerfile, t("Dockerfile を標準に戻しました。"));
  };

  const dockerLabel =
    status?.docker === "ready" ? t("Docker: 使えます") : status?.docker === "stopped" ? t("Docker: 停止中") : t("Docker: 未導入");
  const building = busy === "build" || status?.building;

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
              <li>{t("Dockerfile と sandbox.env はこのエージェントのフォルダにあり、編集できます。")}</li>
            </ul>
          </Heading>
          <p className="text-xs text-muted-foreground">
            {t("コマンドをこのエージェント専用のDockerコンテナで実行します(認証情報の分離用)。")}
          </p>
        </div>
        <Switch checked={enabled ?? false} disabled={enabled === null} onCheckedChange={change} aria-label={t("サンドボックス")} />
      </div>

      {enabled && (
        <div className="flex flex-col gap-4 border-t pt-4">
          <div className="flex flex-wrap items-center gap-2">
            <StatusPill on={status?.docker === "ready"}>{dockerLabel}</StatusPill>
            <StatusPill on={!!status?.image_ready}>{building ? t("イメージ: ビルド中") : status?.image_ready ? t("イメージ: 準備済み") : t("イメージ: 未ビルド")}</StatusPill>
            <StatusPill on={!!status?.container_running}>{status?.container_running ? t("コンテナ: 実行中") : t("コンテナ: 停止")}</StatusPill>
            <Button size="icon-sm" variant="ghost" onClick={() => void refresh()} aria-label={t("状態を更新")}>
              <RefreshCw className="size-3.5" />
            </Button>
          </div>
          {status?.docker === "missing" && (
            <p className="text-xs text-muted-foreground">{t("Dockerが見つかりません。Docker Desktop などをインストールして起動してください。")}</p>
          )}
          {status?.docker === "stopped" && (
            <p className="text-xs text-muted-foreground">{t("Dockerに接続できません。Docker Desktop などを起動してください。")}</p>
          )}

          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="hud-label">Dockerfile</span>
              <Badge variant="outline">
                {files?.dockerfile === "edited" ? t("編集済み") : files?.dockerfile === "template" ? t("標準") : t("未作成")}
              </Badge>
              <Button size="sm" variant="outline" onClick={() => run("reveal-dockerfile", () => revealSandboxFile("dockerfile"))}>
                <FileText className="size-3.5" />
                {t("ファイルを表示")}
              </Button>
              <Button size="sm" variant="outline" disabled={files?.dockerfile !== "edited"} onClick={confirmReset}>
                {t("標準に戻す")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={status?.docker !== "ready" || !!busy || status?.building}
                onClick={() => run("build", () => buildSandboxImage(false), t("イメージを準備しました。"))}
              >
                {building ? <Loader2 className="size-3.5 animate-spin" /> : null}
                {t("イメージをビルド")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={status?.docker !== "ready" || !!busy}
                onClick={() => run("reset", resetSandbox, t("コンテナを作り直します(次のコマンドで作成されます)。"))}
              >
                {t("コンテナを作り直す")}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              {t("Dockerfile を保存すると、次のコマンド実行時に自動で再ビルドされます。")}
            </p>
            {building && log && <p className="truncate font-mono text-[0.7rem] text-muted-foreground">{log}</p>}
          </div>

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
