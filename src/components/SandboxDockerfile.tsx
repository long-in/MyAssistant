"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { FileText, Loader2, RefreshCw } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  SANDBOX_BUILD_EVENT,
  type DockerfileState,
  type SandboxBuildProgress,
  type SandboxImageStatus,
  buildSandboxImage,
  getAgentSandboxDockerfile,
  getSandboxDockerfile,
  getSandboxImageStatus,
  getSandboxStatus,
  registerAgentSandboxDockerfile,
  reportError,
  resetSandboxDockerfile,
  revealSandboxFile,
  setAgentSandboxDockerfile,
  setSandboxDockerfile,
  unregisterAgentSandboxDockerfile,
} from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Heading, StatusPill } from "@/components/GlobalPanels";

/**
 * Dockerイメージを作るDockerfileの管理。
 * - 共通(`agent`なし): 全体設定。全エージェントが使う。
 * - エージェント(`agent`あり): そのエージェント専用のDockerfile。登録があれば共通より優先する。
 *   登録は共通の内容のコピーから始まり、解除すると共通に戻る。
 * 画面内で編集して保存するか、Finderで開いて外部のエディタで編集する(どちらも同じファイル)。
 * 保存したら、Sandbox 設定でイメージを再ビルドし、コンテナを作り直す(チャットは、準備が済むまで使えない)。
 */
export default function SandboxDockerfile({
  agent,
  embedded,
  onChange,
  onDirtyChange,
}: {
  agent?: { id: string };
  embedded?: boolean;
  onChange?: () => void;
  /** 未保存の変更の有無が変わったとき(チェックリストの警告に使う)。 */
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const t = useT();
  const scope = agent?.id ?? "shared";
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [state, setState] = useState<DockerfileState>("missing");
  /** エージェントのとき: 専用のDockerfileが登録されているか。共通では常に登録扱い。 */
  const [registered, setRegistered] = useState(!agent);
  const [ready, setReady] = useState<{ docker: SandboxImageStatus["docker"]; image_ready: boolean; building: boolean } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [log, setLog] = useState("");
  const dirty = registered && saved !== null && draft !== saved;
  const dirtyRef = useRef(false);
  useEffect(() => {
    dirtyRef.current = dirty;
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);

  const refreshStatus = useCallback(async () => {
    try {
      if (agent) {
        const s = await getSandboxStatus();
        setReady({ docker: s.docker, image_ready: s.image_ready, building: s.building });
      } else {
        setReady(await getSandboxImageStatus());
      }
    } catch (e) {
      toast.error(reportError(e));
    }
  }, [agent]);

  /** ファイルの内容を読み直し、編集中の内容は捨てる。 */
  const load = useCallback(async () => {
    try {
      if (agent) {
        const d = await getAgentSandboxDockerfile();
        setRegistered(d.registered);
        setSaved(d.content);
        setDraft(d.content);
        setState(d.registered ? "edited" : "template");
      } else {
        const d = await getSandboxDockerfile();
        setSaved(d.content);
        setDraft(d.content);
        setState(d.state);
      }
    } catch (e) {
      toast.error(reportError(e));
    }
  }, [agent]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load();
    void refreshStatus();
  }, [load, refreshStatus]);

  // 外部のエディタで保存した変更は、画面に戻ったときに取り込む(編集中でなければ)
  useEffect(() => {
    const onFocus = () => {
      if (!dirtyRef.current) void load();
      void refreshStatus();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [load, refreshStatus]);

  // 共通のイメージのビルドは共通の画面だけ、専用のビルドはそのエージェントの画面だけに流す
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
      await refreshStatus();
      onChange?.();
    }
  };

  const save = () =>
    run(
      "save",
      async () => {
        if (agent) {
          await setAgentSandboxDockerfile(draft);
        } else {
          setState(await setSandboxDockerfile(draft));
        }
        setSaved(draft);
      },
      t("Dockerfile を保存しました。チェックリストで、②イメージのビルド → ③コンテナの起動 をやり直してください。"),
    );

  const reset = async () => {
    const yes = await ask(t("Dockerfile を標準の内容で上書きします。編集した内容は失われます。続行しますか?"), {
      title: t("Dockerfile を標準に戻す"),
      kind: "warning",
      okLabel: t("上書きする"),
      cancelLabel: t("キャンセル"),
    });
    if (yes) await run("reset", async () => { await resetSandboxDockerfile(); await load(); }, t("Dockerfile を標準に戻しました。"));
  };

  const register = () =>
    run("register", async () => { await registerAgentSandboxDockerfile(); await load(); }, t("専用の Dockerfile を登録しました。共通の内容がコピーされています。"));

  const unregister = async () => {
    const yes = await ask(t("専用の Dockerfile を削除して、共通の Dockerfile に戻します。編集した内容は失われます。続行しますか?"), {
      title: t("登録を解除"),
      kind: "warning",
      okLabel: t("解除する"),
      cancelLabel: t("キャンセル"),
    });
    if (yes) await run("unregister", async () => { await unregisterAgentSandboxDockerfile(); await load(); }, t("登録を解除しました。共通の Dockerfile を使います。"));
  };

  const building = busy === "build" || !!ready?.building;
  const dockerLabel =
    ready?.docker === "ready" ? t("Docker: 使えます") : ready?.docker === "stopped" ? t("Docker: 停止中") : t("Docker: 未導入");

  const body = (
    <>
      <Heading
        title="Sandbox Image"
        help={agent ? t("このエージェントの Dockerfile") : t("共通のDockerfile")}
        status={<StatusPill on={!!ready?.image_ready}>{building ? t("イメージ: ビルド中") : ready?.image_ready ? t("イメージ: 準備済み") : t("イメージ: 未ビルド")}</StatusPill>}
      >
        {agent ? (
          <>
            <p>{t("専用の Dockerfile を作ると、このエージェントだけ、共通ではなく専用のイメージを使います。なければ共通の Dockerfile を使います。")}</p>
            <ul>
              <li>{t("登録すると、共通の Dockerfile の内容がコピーされます。以後、共通を変更しても専用には反映されません。")}</li>
              <li>{t("画面で編集して保存するか、「Finder で開く」で外部のエディタで編集できます(同じファイルです)。")}</li>
              <li>{t("作成・編集・解除をしたら、上の手順でイメージを再ビルドし、コンテナを作り直してください。専用ボリュームは残ります。")}</li>
              <li>{t("ユーザー agent、sh、setsid(util-linux)、/workspace と /home/agent が必要です。")}</li>
            </ul>
          </>
        ) : (
          <>
            <p>{t("すべてのエージェントのコンテナは、このDockerfileから作る共通のイメージを使います。ただし、専用の Dockerfile を登録したエージェントは、そちらを優先します。")}</p>
            <ul>
              <li>{t("画面で編集して保存するか、「Finder で開く」で外部のエディタで編集できます(同じファイルです)。")}</li>
              <li>{t("保存したら、イメージを再ビルドし、各エージェントのコンテナを作り直してください(エージェントの Sandbox 設定の手順で行います)。専用ボリュームは残ります。")}</li>
              <li>{t("必要なツール(AWS CLI など)はここに入れます。エージェントごとの違いは sandbox.env か、エージェント専用の Dockerfile で与えます。")}</li>
              <li>{t("ユーザー agent、sh、setsid(util-linux)、/workspace と /home/agent が必要です。")}</li>
              <li>{t("環境変数や認証情報はビルドに渡しません。")}</li>
            </ul>
          </>
        )}
      </Heading>

      <div className="flex flex-wrap items-center gap-2">
        {agent ? (
          <Badge variant={registered ? "default" : "outline"}>{registered ? t("専用を使用中") : t("共通を使用中")}</Badge>
        ) : (
          <Badge variant="outline">{state === "edited" ? t("編集済み") : state === "template" ? t("標準") : t("未作成")}</Badge>
        )}
        {/* Docker の状態と更新は、エージェントの画面では上のチェックリスト(①)が示すので出さない */}
        {!embedded && <StatusPill on={ready?.docker === "ready"}>{dockerLabel}</StatusPill>}
        {dirty && <Badge variant="secondary">{t("未保存の変更")}</Badge>}
        {!embedded && (
          <Button size="icon-sm" variant="ghost" onClick={() => void refreshStatus()} aria-label={t("状態を更新")}>
            <RefreshCw className="size-3.5" />
          </Button>
        )}
      </div>

      <Textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        disabled={saved === null}
        readOnly={!registered}
        spellCheck={false}
        className="min-h-64 font-mono text-xs"
        aria-label="Dockerfile"
      />

      <div className="flex flex-wrap items-center gap-2">
        {agent && !registered && (
          <Button size="sm" disabled={!!busy || saved === null} onClick={() => void register()}>
            {t("専用の Dockerfile を作る")}
          </Button>
        )}
        {/* 保存と取り消しは、編集して変更があるときだけ出す。取り消しはファイルから読み直す(外部のエディタでの保存も取り込む) */}
        {dirty && (
          <>
            <Button size="sm" disabled={!!busy} onClick={() => void save()}>
              {t("保存")}
            </Button>
            <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void load()}>
              {t("変更を破棄")}
            </Button>
          </>
        )}
        {registered && (
          <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void run("reveal", () => revealSandboxFile(agent ? "agent_dockerfile" : "dockerfile"))}>
            <FileText className="size-3.5" />
            {t("Finder で開く")}
          </Button>
        )}
        {agent
          ? registered && (
              <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void unregister()}>
                {t("専用をやめて共通を使う")}
              </Button>
            )
          : state !== "template" && (
              <Button size="sm" variant="outline" disabled={!!busy} onClick={() => void reset()}>
                {t("標準に戻す")}
              </Button>
            )}
        {!agent && (
          <Button
            size="sm"
            variant="outline"
            disabled={ready?.docker !== "ready" || !!busy || dirty || building}
            onClick={() => void run("build", () => buildSandboxImage(false), t("イメージを準備しました。"))}
          >
            {building ? <Loader2 className="size-3.5 animate-spin" /> : null}
            {t("イメージをビルド")}
          </Button>
        )}
      </div>
      {agent && !registered && <p className="text-xs text-muted-foreground">{t("登録がないので、共通の Dockerfile を使います(表示は共通の内容で、編集できません)。")}</p>}
      {dirty && <p className="text-xs text-muted-foreground">{t("ビルドは保存した内容で行います。先に保存してください。")}</p>}
      {building && log && <p className="truncate font-mono text-[0.7rem] text-muted-foreground">{log}</p>}
    </>
  );

  return embedded ? <div className="flex flex-col gap-3 border-t pt-4">{body}</div> : <section className="hud-panel hud-accent flex flex-col gap-3 px-5 py-4">{body}</section>;
}
