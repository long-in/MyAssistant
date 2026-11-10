"use client";

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Check, Loader2, RefreshCw } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  SANDBOX_BUILD_EVENT,
  type SandboxBuildProgress,
  type SandboxStatus,
  buildAgentSandboxImage,
  reportError,
  resetSandbox,
  startAgentSandbox,
  stopAgentSandbox,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type StepState = "done" | "active" | "running" | "blocked";

/**
 * チャットを使うための準備の手順。①Docker の確認 → ②イメージのビルド → ③コンテナの起動 を順に行う。
 * 前の手順が済むまで、次の手順は押せない。すべて済むとチャットが使える。
 */
export default function SandboxSteps({ workspaceId, status, onChange }: { workspaceId: string; status: SandboxStatus | null; onChange: () => Promise<void> }) {
  const t = useT();
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState("");

  // 専用のイメージのビルドはそのエージェントのID、共通のイメージは "shared" で流れてくる
  const scope = status?.dockerfile_registered ? workspaceId : "shared";
  useEffect(() => {
    const un = listen<SandboxBuildProgress>(SANDBOX_BUILD_EVENT, (e) => {
      if (e.payload.scope === scope) setLog(e.payload.line);
    });
    return () => {
      un.then((f) => f());
    };
  }, [scope]);

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
      await onChange();
    }
  };

  const dockerOk = status?.docker === "ready";
  const building = busy === "build" || !!status?.building;
  const imageOk = dockerOk && !!status?.image_ready;
  const containerOk = imageOk && !!status?.container_ready;

  const s1: StepState = dockerOk ? "done" : "active";
  const s2: StepState = building ? "running" : imageOk ? "done" : dockerOk ? "active" : "blocked";
  const s3: StepState = busy === "start" ? "running" : containerOk ? "done" : imageOk ? "active" : "blocked";

  const dockerNote =
    status?.docker === "missing"
      ? t("Dockerが見つかりません。Docker Desktop などをインストールして起動してください。")
      : status?.docker === "stopped"
        ? t("Dockerに接続できません。Docker Desktop などを起動してください。")
        : status
          ? t("Docker が使えます。")
          : "…";

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <span className="hud-label">{t("チャットを使うための準備")}</span>
        <Button size="icon-sm" variant="ghost" onClick={() => void onChange()} aria-label={t("状態を更新")}>
          <RefreshCw className="size-3.5" />
        </Button>
      </div>
      <ol className="flex flex-col">
        <Step n={1} state={s1} title={t("Docker を確認")} last={false}>
          <p className="text-xs text-muted-foreground">{dockerNote}</p>
        </Step>
        <Step n={2} state={s2} title={t("イメージをビルド")} last={false}>
          <p className="text-xs text-muted-foreground">
            {building
              ? t("ビルド中です。初回は数分かかります。")
              : imageOk
                ? t("ビルド済みです。Dockerfile を変更したときは、再ビルドしてください。")
                : t("Dockerfile から、コンテナの元になるイメージを作ります。")}
          </p>
          {building && log && <p className="truncate font-mono text-[0.7rem] text-muted-foreground">{log}</p>}
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant={imageOk ? "outline" : "default"}
              disabled={!dockerOk || !!busy || building}
              onClick={() => void run("build", () => buildAgentSandboxImage(imageOk), t("イメージをビルドしました。"))}
            >
              {building && <Loader2 className="size-3.5 animate-spin" />}
              {imageOk ? t("再ビルド") : t("イメージをビルド")}
            </Button>
          </div>
        </Step>
        <Step n={3} state={s3} title={t("コンテナを起動")} last>
          <p className="text-xs text-muted-foreground">
            {containerOk
              ? t("起動中です。チャットを使えます。")
              : status?.container_running && imageOk
                ? t("起動中のコンテナが古いイメージのままです。作り直してください。")
                : t("このエージェント専用のコンテナを起動します。起動するとチャットを使えます。")}
          </p>
          {status?.container_name && containerOk && (
            <p className="text-xs text-muted-foreground">
              {t("コンテナ名")}: <span className="font-mono">{status.container_name}</span>
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            {containerOk ? (
              <>
                <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void run("stop", stopAgentSandbox, t("コンテナを停止しました。"))}>
                  {t("停止")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!!busy}
                  onClick={() => void run("start", async () => { await resetSandbox(); await startAgentSandbox(); }, t("コンテナを作り直しました。"))}
                >
                  {t("コンテナを作り直す")}
                </Button>
              </>
            ) : (
              <Button
                size="sm"
                disabled={!imageOk || !!busy}
                onClick={() => void run("start", async () => { if (status?.container_running) await resetSandbox(); await startAgentSandbox(); }, t("コンテナを起動しました。"))}
              >
                {busy === "start" && <Loader2 className="size-3.5 animate-spin" />}
                {status?.container_running ? t("コンテナを作り直す") : t("コンテナを起動")}
              </Button>
            )}
          </div>
        </Step>
      </ol>
    </div>
  );
}

function Step({ n, state, title, last, children }: { n: number; state: StepState; title: string; last: boolean; children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <div className="flex flex-col items-center">
        <span
          className={cn(
            "flex size-6 shrink-0 items-center justify-center rounded-full border text-xs font-medium",
            state === "done" && "border-primary bg-primary text-primary-foreground",
            (state === "active" || state === "running") && "border-primary text-primary",
            state === "blocked" && "border-border text-muted-foreground",
          )}
        >
          {state === "done" ? <Check className="size-3.5" /> : state === "running" ? <Loader2 className="size-3.5 animate-spin" /> : n}
        </span>
        {!last && <span className={cn("my-1 w-px flex-1", state === "done" ? "bg-primary/60" : "bg-border")} />}
      </div>
      <div className={cn("flex min-w-0 flex-1 flex-col gap-1.5", last ? "pb-0" : "pb-4", state === "blocked" && "opacity-60")}>
        <span className="text-sm font-medium">{title}</span>
        {children}
      </div>
    </li>
  );
}
