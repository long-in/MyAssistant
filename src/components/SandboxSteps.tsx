"use client";

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Check, Loader2, RefreshCw, RotateCcw } from "lucide-react";
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

/** done: 済み / active: 次にやる / running: 実行中 / blocked: 前の手順待ち / redo: 変更があり、やり直しが必要 */
type StepState = "done" | "active" | "running" | "blocked" | "redo";

const WARN = "border-amber-500/60 bg-amber-500/10 text-amber-700 dark:text-amber-300";

/**
 * チャットを使うための準備のチェックリスト。①Docker の確認 → ②イメージのビルド → ③コンテナの起動 を順に行う。
 * 前の手順が済むまで、次の手順は押せない。すべて済むとチャットが使える。
 * Dockerfile を変えた・イメージを再ビルドしたときは、反映が必要な手順を「要やり直し」にして、上のバナーでも知らせる。
 * ②をやり直したら、③も作り直さないと新しいイメージにならないので、③も「要やり直し」になる。
 */
export default function SandboxSteps({
  workspaceId,
  status,
  onChange,
  dockerfileDirty,
  containerStale,
  onRebuilt,
  onContainerRefreshed,
  onEditDockerfile,
}: {
  workspaceId: string;
  status: SandboxStatus | null;
  onChange: () => Promise<void>;
  /** Dockerfile に未保存の変更があるか。 */
  dockerfileDirty: boolean;
  /** イメージを再ビルドしたあと、コンテナをまだ作り直していないか。 */
  containerStale: boolean;
  /** 起動中のコンテナがあるまま、イメージをビルドし直した。 */
  onRebuilt: () => void;
  /** コンテナを作り直した(または停止した)。 */
  onContainerRefreshed: () => void;
  /** 詳細設定を開いて Dockerfile の編集へ移る。 */
  onEditDockerfile: () => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState("");
  const [errors, setErrors] = useState<Record<string, string>>({});

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

  const run = async (key: string, fn: () => Promise<void>, ok?: string, after?: () => void) => {
    setBusy(key);
    setErrors((p) => ({ ...p, [key]: "" }));
    try {
      await fn();
      after?.();
      if (ok) toast.success(ok);
    } catch (e) {
      const msg = reportError(e);
      setErrors((p) => ({ ...p, [key]: msg }));
      toast.error(msg);
    } finally {
      setBusy(null);
      setLog("");
      await onChange();
    }
  };

  const dockerOk = status?.docker === "ready";
  const building = busy === "build" || !!status?.building;
  const imageOk = dockerOk && !!status?.image_ready;
  const containerExists = !!status?.container_running;
  /** コンテナが、いまのイメージから作られた最新のものか。 */
  const containerOk = imageOk && !!status?.container_ready && !containerStale;
  /** 以前に準備を済ませている(コンテナがある)のに、イメージが無い = Dockerfile が変わった。 */
  const imageOutdated = dockerOk && !imageOk && !building && !!status?.container_name;

  const s1: StepState = dockerOk ? "done" : "active";
  const s2: StepState = building ? "running" : !dockerOk ? "blocked" : imageOk ? "done" : imageOutdated ? "redo" : "active";
  const s3: StepState = busy === "start" ? "running" : containerOk ? "done" : containerExists && dockerOk ? "redo" : imageOk ? "active" : "blocked";
  const states = [s1, s2, s3];
  const doneCount = states.filter((s) => s === "done").length;
  const names = [t("Docker を確認"), t("イメージをビルド"), t("コンテナを起動")];
  const marks = ["①", "②", "③"];
  const redoNames = states.flatMap((s, i) => (s === "redo" ? [`${marks[i]} ${names[i]}`] : []));
  const nextIdx = states.findIndex((s) => s !== "done");

  const redoReason = imageOutdated
    ? t("Dockerfile が変更されたため、イメージが古くなっています。")
    : containerStale
      ? t("イメージを再ビルドしたため、起動中のコンテナが古いままです。")
      : t("起動中のコンテナが古いイメージのままです。");

  // 上部の結論。やり直しがあれば最優先で警告にする。
  let banner: { tone: "ok" | "warn" | "todo"; text: string; sub?: string };
  if (redoNames.length > 0) {
    banner = {
      tone: "warn",
      text: t("設定の変更が反映されていません。やり直してください: ") + redoNames.join(" → "),
      sub: redoReason,
    };
  } else if (dockerfileDirty) {
    banner = {
      tone: "warn",
      text: t("Dockerfile に未保存の変更があります。保存してから、②イメージのビルド → ③コンテナの起動 をやり直してください。"),
    };
  } else if (nextIdx === -1) {
    banner = { tone: "ok", text: t("すべて完了しました。チャットを使えます。") };
  } else {
    banner = { tone: "todo", text: t("チャットはまだ使えません。次にやること: ") + `${marks[nextIdx]} ${names[nextIdx]}` };
  }

  const dockerNote =
    status?.docker === "missing"
      ? t("Dockerが見つかりません。Docker Desktop などをインストールして起動してください。")
      : status?.docker === "stopped"
        ? t("Dockerに接続できません。Docker Desktop などを起動してください。")
        : status
          ? t("Docker が使えます。")
          : "…";
  const dockerNow = status?.docker === "ready" ? t("接続OK") : status?.docker === "stopped" ? t("停止中") : status?.docker === "missing" ? t("未導入") : "…";
  const imageKind = status?.dockerfile_registered ? t("専用の Dockerfile") : t("共通の Dockerfile");
  const imageNow = building ? t("ビルド中") : imageOk ? `${t("ビルド済み")}(${imageKind})` : imageOutdated ? t("古い(再ビルドが必要)") : t("未ビルド");
  const containerNow = containerOk
    ? t("起動中")
    : containerExists
      ? t("古い(作り直しが必要)")
      : status?.container_name
        ? t("停止中")
        : t("未作成");

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <span className="hud-label">{t("チャットを使うための準備")}</span>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">
            {doneCount} / 3 {t("完了")}
          </span>
          <Button size="icon-sm" variant="ghost" onClick={() => void onChange()} aria-label={t("状態を更新")}>
            <RefreshCw className="size-3.5" />
          </Button>
        </div>
      </div>

      <div className="h-1.5 overflow-hidden rounded-full bg-border" role="progressbar" aria-valuemin={0} aria-valuemax={3} aria-valuenow={doneCount}>
        <div className={cn("h-full transition-all", redoNames.length ? "bg-amber-500" : "bg-primary")} style={{ width: `${(doneCount / 3) * 100}%` }} />
      </div>

      <div
        role="status"
        className={cn(
          "rounded-md border px-3 py-2 text-sm font-medium",
          banner.tone === "warn" && WARN,
          banner.tone === "ok" && "border-primary/50 bg-primary/10 text-primary",
          banner.tone === "todo" && "border-border bg-muted/40",
        )}
      >
        {banner.tone === "ok" && "✅ "}
        {banner.tone === "warn" && "⚠ "}
        {banner.text}
        {banner.sub && <p className="mt-0.5 text-xs font-normal opacity-90">{banner.sub}</p>}
      </div>

      <ol className="flex flex-col">
        <Step n={1} state={s1} title={names[0]} now={dockerNow} last={false}>
          <p className="text-xs text-muted-foreground">{dockerNote}</p>
        </Step>
        <Step n={2} state={s2} title={names[1]} now={imageNow} last={false}>
          <p className="text-xs text-muted-foreground">
            {building
              ? t("ビルド中です。初回は数分かかります。")
              : imageOk
                ? t("Dockerfile を変更したときは、再ビルドしてください。")
                : t("Dockerfile から、コンテナの元になるイメージを作ります。")}
          </p>
          {building && log && <p className="truncate font-mono text-[0.7rem] text-muted-foreground">{log}</p>}
          {errors.build && <p className="text-xs text-destructive">{errors.build}</p>}
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant={s2 === "done" ? "outline" : "default"}
              disabled={!dockerOk || !!busy || building}
              onClick={() =>
                void run("build", () => buildAgentSandboxImage(imageOk), t("イメージをビルドしました。"), () => {
                  if (containerExists) onRebuilt();
                })
              }
            >
              {building && <Loader2 className="size-3.5 animate-spin" />}
              {imageOk ? t("再ビルド") : errors.build ? t("もう一度ビルド") : t("イメージをビルド")}
            </Button>
            <Button size="sm" variant="link" className="px-1" onClick={onEditDockerfile}>
              {t("Dockerfile を編集")}
            </Button>
          </div>
        </Step>
        <Step n={3} state={s3} title={names[2]} now={containerNow} last>
          <p className="text-xs text-muted-foreground">
            {containerOk
              ? t("起動中です。チャットを使えます。")
              : s3 === "redo" && !imageOk
                ? t("②のイメージをビルドし直したあとで、コンテナを作り直してください。")
                : s3 === "redo"
                  ? t("新しいイメージで、コンテナを作り直してください。")
                  : t("このエージェント専用のコンテナを起動します。起動するとチャットを使えます。")}
          </p>
          {status?.container_name && containerOk && (
            <p className="text-xs text-muted-foreground">
              {t("コンテナ名")}: <span className="font-mono">{status.container_name}</span>
            </p>
          )}
          {errors.start && <p className="text-xs text-destructive">{errors.start}</p>}
          {errors.stop && <p className="text-xs text-destructive">{errors.stop}</p>}
          <div className="flex flex-wrap gap-2">
            {containerOk ? (
              <>
                <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void run("stop", stopAgentSandbox, t("コンテナを停止しました。"), onContainerRefreshed)}>
                  {t("停止")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!!busy}
                  onClick={() => void run("start", async () => { await resetSandbox(); await startAgentSandbox(); }, t("コンテナを作り直しました。"), onContainerRefreshed)}
                >
                  {t("コンテナを作り直す")}
                </Button>
              </>
            ) : (
              <Button
                size="sm"
                disabled={!imageOk || !!busy || building}
                onClick={() => void run("start", async () => { if (containerExists) await resetSandbox(); await startAgentSandbox(); }, t("コンテナを起動しました。"), onContainerRefreshed)}
              >
                {busy === "start" && <Loader2 className="size-3.5 animate-spin" />}
                {containerExists ? t("コンテナを作り直す") : t("コンテナを起動")}
              </Button>
            )}
          </div>
        </Step>
      </ol>
    </div>
  );
}

function Step({ n, state, title, now, last, children }: { n: number; state: StepState; title: string; now: string; last: boolean; children: React.ReactNode }) {
  const t = useT();
  return (
    <li className="flex gap-3">
      <div className="flex flex-col items-center">
        <span
          className={cn(
            "flex size-6 shrink-0 items-center justify-center rounded-full border text-xs font-medium",
            state === "done" && "border-primary bg-primary text-primary-foreground",
            (state === "active" || state === "running") && "border-primary text-primary",
            state === "redo" && "border-amber-500 bg-amber-500 text-white",
            state === "blocked" && "border-border text-muted-foreground",
          )}
        >
          {state === "done" ? <Check className="size-3.5" /> : state === "running" ? <Loader2 className="size-3.5 animate-spin" /> : state === "redo" ? <RotateCcw className="size-3.5" /> : n}
        </span>
        {!last && <span className={cn("my-1 w-px flex-1", state === "done" ? "bg-primary/60" : "bg-border")} />}
      </div>
      <div className={cn("flex min-w-0 flex-1 flex-col gap-1.5", last ? "pb-0" : "pb-4", state === "blocked" && "opacity-60")}>
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium">{title}</span>
          {state === "redo" && <span className={cn("rounded-full border px-2 py-0.5 text-[0.65rem] font-semibold", WARN)}>{t("要やり直し")}</span>}
          {state === "active" && <span className="rounded-full border border-primary/50 px-2 py-0.5 text-[0.65rem] font-semibold text-primary">{t("次にやる")}</span>}
        </div>
        <p className="text-xs">
          <span className="text-muted-foreground">{t("いまの状態")}: </span>
          <span className={cn("font-medium", state === "redo" && "text-amber-700 dark:text-amber-300")}>{now}</span>
        </p>
        {children}
      </div>
    </li>
  );
}
