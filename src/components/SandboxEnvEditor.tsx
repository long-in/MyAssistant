"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { FileText } from "lucide-react";
import { useT } from "@/lib/i18n";
import { type SandboxEnvSummary, getSandboxEnv, reportError, revealSandboxFile, setSandboxEnv } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

/**
 * このエージェントの sandbox.env(コンテナへ渡す環境変数)のエディタ。値を含めてそのまま表示・編集する。
 * 保存時に書式を検証し、誤りがあれば保存しない。外部のエディタで保存した変更は、画面へ戻ったときに取り込む(編集中でなければ)。
 */
export default function SandboxEnvEditor() {
  const t = useT();
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [summary, setSummary] = useState<SandboxEnvSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const dirty = saved !== null && draft !== saved;
  const dirtyRef = useRef(false);
  useEffect(() => {
    dirtyRef.current = dirty;
  }, [dirty]);

  const load = useCallback(async () => {
    try {
      const f = await getSandboxEnv();
      setSaved(f.content);
      setDraft(f.content);
      setSummary(f.summary);
    } catch (e) {
      toast.error(reportError(e));
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load();
    const onFocus = () => {
      if (!dirtyRef.current) void load();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [load]);

  const save = async () => {
    setBusy(true);
    try {
      setSummary(await setSandboxEnv(draft));
      setSaved(draft);
      toast.success(t("保存しました。次の指示から反映されます。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  const reload = async () => {
    const ok =
      !dirty ||
      (await ask(t("編集中の内容は破棄されます。ファイルから読み込み直しますか?"), {
        title: t("再読み込み"),
        kind: "warning",
        okLabel: t("読み込む"),
        cancelLabel: t("キャンセル"),
      }));
    if (ok) await load();
  };

  const reveal = async () => {
    try {
      await revealSandboxFile("env");
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="hud-label">sandbox.env</span>
        {dirty && <Badge variant="secondary">{t("未保存の変更")}</Badge>}
      </div>
      <Textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        disabled={saved === null}
        spellCheck={false}
        autoComplete="off"
        className="min-h-40 font-mono text-xs"
        aria-label="sandbox.env"
      />
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={!dirty || busy} onClick={() => void save()}>
          {t("保存")}
        </Button>
        <Button size="sm" variant="outline" disabled={!dirty || busy} onClick={() => setDraft(saved ?? "")}>
          {t("変更を破棄")}
        </Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void reload()}>
          {t("再読み込み")}
        </Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void reveal()}>
          <FileText className="size-3.5" />
          {t("ファイルを表示")}
        </Button>
      </div>
      {summary?.error ? (
        <p className="text-xs text-destructive">
          {summary.error.line != null
            ? t("{line}行目が正しくありません: {message}", { line: summary.error.line, message: summary.error.message })
            : summary.error.message}
        </p>
      ) : summary && summary.names.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs text-muted-foreground">{t("渡す変数")}:</span>
          {summary.names.map((n) => (
            <Badge key={n} variant="secondary" className="font-mono">
              {n}
            </Badge>
          ))}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">{t("値が入った変数はまだありません。")}</p>
      )}
      <p className="text-xs text-muted-foreground">
        {t("NAME=value の形式で1行に1つ書きます。値が空の行はコンテナへ渡しません。AWS の認証情報は AWS の設定から渡すので、ここには不要です。")}
      </p>
      <p className="text-xs text-muted-foreground">{t("値(API キーなど)はこの画面にそのまま表示されます。画面を共有するときは注意してください。")}</p>
    </div>
  );
}
