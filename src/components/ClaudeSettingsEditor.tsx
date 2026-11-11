"use client";

import { useCallback, useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Eye, EyeOff } from "lucide-react";
import { useT } from "@/lib/i18n";
import { getClaudeSettingsTemplate, getSandboxClaudeSettings, reportError, setSandboxClaudeSettings } from "@/lib/api";
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
 * モデル(`model`)と許可・拒否するツール(`permissions.allow` / `permissions.deny`)もここで決める。空のときは、雛形を入れるボタンを出す。
 * `env` に値があるときは、既定で値を伏せた読み取り専用の表示にし、「値を表示して編集」で本文を出す。
 */
export default function ClaudeSettingsEditor() {
  const t = useT();
  // 実際に使われる内容(自分の保存があればそれ、なければ全体設定)。`own`が空なら全体設定を利用している
  const [saved, setSaved] = useState<string | null>(null);
  const [own, setOwn] = useState("");
  const [shared, setShared] = useState("");
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [template, setTemplate] = useState<string | null>(null);
  const dirty = saved !== null && draft.trim() !== saved.trim();
  const masked = revealed ? null : maskEnv(draft);

  const load = useCallback(async () => {
    try {
      const s = await getSandboxClaudeSettings();
      const effective = s.own.trim() ? s.own : s.shared;
      setOwn(s.own);
      setShared(s.shared);
      setSaved(effective);
      setDraft(effective);
      setError(null);
    } catch (e) {
      toast.error(reportError(e));
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load();
  }, [load]);

  useEffect(() => {
    getClaudeSettingsTemplate()
      .then(setTemplate)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const save = async () => {
    setBusy(true);
    try {
      await setSandboxClaudeSettings(draft);
      // 全体設定と同じ内容、または空は、全体設定を利用する状態になる(個別設定を持たない)
      await load();
      toast.success(t("保存しました。次の指示から反映されます。"));
    } catch (e) {
      setError(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  /** 個別設定をやめて全体設定を利用する(以後は全体設定の変更に追従する)。 */
  const switchToShared = async () => {
    const yes = await ask(t("このエージェント専用の settings.json を破棄して、全体設定の settings.json を利用します。よろしいですか?"), {
      title: t("全体設定を利用"),
      kind: "warning",
      okLabel: t("利用する"),
      cancelLabel: t("キャンセル"),
    });
    if (!yes) return;
    setBusy(true);
    try {
      await setSandboxClaudeSettings("");
      await load();
      toast.success(t("全体設定を利用します。"));
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
        {saved !== null && <Badge variant={own.trim() ? "secondary" : "outline"}>{own.trim() ? t("個別設定") : t("全体設定を利用")}</Badge>}
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
        {own.trim() !== "" && (
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void switchToShared()}>
            {t("全体設定を利用")}
          </Button>
        )}
        {(masked !== null || (revealed && maskEnv(draft) !== null)) && (
          <Button size="sm" variant="outline" onClick={() => setRevealed((v) => !v)}>
            {revealed ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
            {revealed ? t("値を伏せる") : t("値を表示して編集")}
          </Button>
        )}
      </div>
      {saved !== null && saved.trim() === "" && shared.trim() === "" && (
        <div className="flex flex-col gap-2 border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
          <p>{t("settings.json が空のため、Bash・Read などのツールは許可されていません(指示を実行しても、ツールを使えません)。拒否は、rm・sudo などの初期値だけを渡します。許可・拒否を書いた雛形を入れてから、保存してください。")}</p>
          <div>
            <Button size="sm" variant="outline" disabled={draft.trim() !== "" || template === null} onClick={() => template && setDraft(template)}>
              {t("許可・拒否の雛形を入れる")}
            </Button>
          </div>
        </div>
      )}
      <p className="text-xs text-muted-foreground">
        {own.trim() ? t("このエージェント専用の内容です。全体設定が変わっても更新されません。") : t("全体設定の内容を利用しています。全体設定が変わると、次の指示から自動で反映されます。編集して保存すると、このエージェント専用の個別設定になります。")}
      </p>
      {masked !== null && <p className="text-xs text-muted-foreground">{t("env の値は伏せています。編集するには「値を表示して編集」を押してください。")}</p>}
      {error && <p className="text-xs text-destructive">{error}</p>}
      <p className="text-xs text-muted-foreground">
        {t("Claude Code の settings.json の内容を貼り付けます(JSON のオブジェクト)。コンテナ内の Claude Code へ、指示ごとに --settings で渡します。モデルは model、許可するツールは permissions.allow、拒否するツールは permissions.deny(Bash(git status:*) のような指定も可)で決めます。")}
      </p>
      <p className="text-xs text-muted-foreground">
        {t("コンテナ内の Claude Code のコマンドの拒否は、アプリの Command Policy ではなく、ここの permissions.deny で決めます(雛形に rm・sudo などの初期値が入っています)。消すとその保護は無くなります。env の値は、画面では既定で伏せますが、バックアップ・設定の書き出しにはそのまま含まれます。コンテナの出力に値が出たときは伏せ字にします。")}
      </p>
    </div>
  );
}
