"use client";

import { useCallback, useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Eye, EyeOff } from "lucide-react";
import { useT } from "@/lib/i18n";
import { getSandboxClaudeSettings, reportError, setSandboxClaudeSettings } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

const MASKED = "••••••••";

/** `env`の値を伏せた表示用の文字列。`env`に値がない・JSON として読めないときは `null`(伏せる対象がない)。 */
function maskEnv(text: string): string | null {
  try {
    const root: unknown = JSON.parse(text);
    if (typeof root !== "object" || root === null || Array.isArray(root)) return null;
    const env = (root as { env?: unknown }).env;
    if (typeof env !== "object" || env === null || Array.isArray(env) || Object.keys(env).length === 0) return null;
    const masked = Object.fromEntries(Object.keys(env).map((k) => [k, MASKED]));
    return JSON.stringify({ ...root, env: masked }, null, 2);
  } catch {
    return null;
  }
}

/**
 * このエージェントの Claude Code の settings.json を貼り付けるエディタ。保存時に JSON のオブジェクトか検証し、誤りがあれば保存しない。
 * コンテナへは、指示ごとに `--settings` で渡す(ファイルとしては置かない)。空で保存すると未設定に戻る。
 * `env` に値があるときは、既定で値を伏せた読み取り専用の表示にし、「値を表示して編集」で本文を出す。
 */
export default function ClaudeSettingsEditor() {
  const t = useT();
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const dirty = saved !== null && draft.trim() !== saved.trim();
  const masked = revealed ? null : maskEnv(draft);

  const load = useCallback(async () => {
    try {
      const s = await getSandboxClaudeSettings();
      setSaved(s);
      setDraft(s);
      setError(null);
    } catch (e) {
      toast.error(reportError(e));
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load();
  }, [load]);

  const save = async () => {
    setBusy(true);
    try {
      await setSandboxClaudeSettings(draft);
      setSaved(draft.trim());
      setDraft(draft.trim());
      setError(null);
      toast.success(t("保存しました。次の指示から反映されます。"));
    } catch (e) {
      setError(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  const reload = async () => {
    const ok =
      !dirty ||
      (await ask(t("編集中の内容は破棄されます。保存済みの内容を読み込み直しますか?"), {
        title: t("再読み込み"),
        kind: "warning",
        okLabel: t("読み込む"),
        cancelLabel: t("キャンセル"),
      }));
    if (ok) await load();
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="hud-label">Claude Code settings.json</span>
        {dirty && <Badge variant="secondary">{t("未保存の変更")}</Badge>}
      </div>
      <Textarea
        value={masked ?? draft}
        onChange={(e) => setDraft(e.target.value)}
        disabled={saved === null}
        readOnly={masked !== null}
        spellCheck={false}
        autoComplete="off"
        placeholder={'{\n  "permissions": { "deny": [] }\n}'}
        className="min-h-40 font-mono text-xs"
        aria-label="Claude Code settings.json"
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
        {(masked !== null || (revealed && maskEnv(draft) !== null)) && (
          <Button size="sm" variant="outline" onClick={() => setRevealed((v) => !v)}>
            {revealed ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
            {revealed ? t("値を伏せる") : t("値を表示して編集")}
          </Button>
        )}
      </div>
      {masked !== null && <p className="text-xs text-muted-foreground">{t("env の値は伏せています。編集するには「値を表示して編集」を押してください。")}</p>}
      {error && <p className="text-xs text-destructive">{error}</p>}
      <p className="text-xs text-muted-foreground">
        {t("Claude Code の settings.json の内容を貼り付けます(JSON のオブジェクト)。コンテナ内の Claude Code へ、指示ごとに --settings で渡します。空で保存すると使いません。")}
      </p>
      <p className="text-xs text-muted-foreground">
        {t("アプリの「コマンド権限」の拒否は、settings.json では緩められません(許可は追加されます)。env の値は、画面では既定で伏せますが、バックアップ・設定の書き出しにはそのまま含まれます。コンテナの出力に値が出たときは伏せ字にします。")}
      </p>
    </div>
  );
}
