"use client";

import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { tr } from "@/lib/i18n";
import {
  getSandboxEnabled,
  getSandboxModels,
  getSandboxStatus,
  reportError,
  resetSandbox,
  setSandboxModel,
  startAgentSandbox,
  stopAgentSandbox,
  type SandboxModels,
  type SandboxStatus,
} from "@/lib/api";

/**
 * 対話画面で使う、サンドボックス(コンテナ内の Claude Code)の状態と操作。
 * `selected`は、モデルにコンテナ内の Claude Code を選んでいるか(パーソナル・リーダー)。ワークのエージェントは常にコンテナで会話する。
 * 状態はアクティブなエージェントのものだけが取れるので、`active`(対話画面を表示中)のあいだだけ見張る。
 */
export function useSandbox(selected: boolean, active: boolean) {
  /** ワークのエージェントは常に、会話をコンテナ内の Claude Code が受け持つ(このアプリのAPIキーやモデルの選択は使わない)。 */
  const [always, setAlways] = useState(false);
  useEffect(() => {
    getSandboxEnabled().then(setAlways).catch(() => {});
  }, []);
  const on = always || selected;
  /** コンテナが使える状態か(未確認は null)。使えるまでは、指示を送れない。 */
  const [ready, setReady] = useState<boolean | null>(null);
  const [status, setStatus] = useState<SandboxStatus | null>(null);
  /** チャット画面からのコンテナの起動・停止の実行中。 */
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(
    () =>
      getSandboxStatus()
        .then((s) => {
          setStatus(s);
          setReady(s.docker === "ready" && s.image_ready && s.container_ready);
        })
        .catch(() => {}),
    [],
  );
  useEffect(() => {
    if (!on || !active) return;
    let alive = true;
    const check = () => (alive ? refresh() : Promise.resolve());
    void check();
    const timer = setInterval(() => void check(), ready ? 30_000 : 5_000);
    window.addEventListener("focus", check);
    return () => {
      alive = false;
      clearInterval(timer);
      window.removeEventListener("focus", check);
    };
  }, [on, active, ready, refresh]);

  /** 選べるモデル(settings.json の許可)。設定画面で書き換えたあと戻ってきたときに読み直す。 */
  const [models, setModels] = useState<SandboxModels | null>(null);
  useEffect(() => {
    if (!on || !active) return;
    let alive = true;
    const load = () =>
      getSandboxModels()
        .then((m) => alive && setModels(m))
        .catch(() => {});
    void load();
    window.addEventListener("focus", load);
    return () => {
      alive = false;
      window.removeEventListener("focus", load);
    };
  }, [on, active]);

  const fail = (e: unknown) => toast.error(reportError(e));
  /** 使うモデルを変える。`null`は settings.json の model に任せる。 */
  const changeModel = (model: string | null) => {
    setModels((m) => (m ? { ...m, selected: model } : m));
    setSandboxModel(model).catch(fail);
  };
  /** コンテナを起動する。起動中なら停止する。 */
  const toggle = async () => {
    if (busy) return;
    setBusy(true);
    try {
      if (ready) {
        await stopAgentSandbox();
        toast.success(tr("コンテナを停止しました。"));
      } else {
        if (status?.container_name && !status.container_ready) await resetSandbox(); // 古いコンテナは作り直す
        await startAgentSandbox();
        toast.success(tr("コンテナを起動しました。"));
      }
      await refresh();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };
  return {
    /** ワークのエージェント(常にコンテナで会話する)か。 */
    always,
    /** コンテナ内の Claude Code で会話するか。 */
    on,
    ready,
    /** コンテナが必要なのに、使える状態でない。 */
    needed: on && ready === false,
    busy,
    /** イメージがあり、起動できる状態か。 */
    startable: !!status && status.docker === "ready" && status.image_ready && !status.building,
    models,
    changeModel,
    toggle,
  };
}
