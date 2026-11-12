"use client";

import { useCallback, useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { useT } from "@/lib/i18n";
import { getClaudeSettingsTemplate, getSandboxClaudeSettings, reportError, setSandboxClaudeSettings } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

/**
 * このエージェントの Claude Code の settings.json を貼り付けるエディタ。保存時に JSON のオブジェクトか検証し、誤りがあれば保存しない。
 * コンテナへは、指示ごとに `--settings` で渡す(ファイルとしては置かない)。空で保存すると未設定に戻る。
 * モデル(`model`)と許可・拒否するツール(`permissions.allow` / `permissions.deny`)もここで決める。空のときは、雛形を入れるボタンを出す。
 * 既定は閲覧だけで、「編集」で書き換えられるようにする。「全体設定に戻す」で、編集中の内容と個別設定を捨てて全体設定へ戻す。
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
  const [editing, setEditing] = useState(false);
  const [template, setTemplate] = useState<string | null>(null);
  const dirty = saved !== null && draft.trim() !== saved.trim();

  const load = useCallback(async () => {
    try {
      const s = await getSandboxClaudeSettings();
      const effective = s.own.trim() ? s.own : s.shared;
      setOwn(s.own);
      setShared(s.shared);
      setSaved(effective);
      setDraft(effective);
      setEditing(false);
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

  /** 編集中の内容と個別設定を捨てて、全体設定を利用する(以後は全体設定の変更に追従する)。 */
  const revertToShared = async () => {
    const yes = await ask(
      own.trim()
        ? t("このエージェント専用の settings.json を破棄して、全体設定の settings.json を利用します。よろしいですか?")
        : t("編集中の内容を破棄して、全体設定の内容に戻します。よろしいですか?"),
      { title: t("全体設定に戻す"), kind: "warning", okLabel: t("戻す"), cancelLabel: t("キャンセル") },
    );
    if (!yes) return;
    setBusy(true);
    try {
      if (own.trim()) await setSandboxClaudeSettings("");
      await load();
      toast.success(t("全体設定を利用します。"));
    } catch (e) {
      setError(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="hud-label">Claude Code settings.json</span>
        {saved !== null && <Badge variant={own.trim() ? "secondary" : "outline"}>{own.trim() ? t("個別設定") : t("全体設定を利用")}</Badge>}
        {dirty && <Badge variant="secondary">{t("未保存の変更")}</Badge>}
      </div>
      <Textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        disabled={saved === null}
        readOnly={!editing}
        spellCheck={false}
        autoComplete="off"
        placeholder={'{\n  "permissions": { "deny": [] }\n}'}
        className="min-h-40 font-mono text-xs"
        aria-label="Claude Code settings.json"
      />
      <div className="flex flex-wrap items-center gap-2">
        {!editing && (
          <Button size="sm" variant="outline" disabled={saved === null || busy} onClick={() => setEditing(true)} title={t("settings.json を書き換えられるようにします")}>
            {t("編集")}
          </Button>
        )}
        <Button size="sm" disabled={!dirty || busy} onClick={() => void save()} title={t("このエージェント専用の内容として保存します(次の指示から反映)")}>
          {t("保存")}
        </Button>
        {(own.trim() !== "" || dirty) && (
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void revertToShared()} title={t("編集中の内容と個別設定を捨てて、全体設定の内容を使います")}>
            {t("全体設定に戻す")}
          </Button>
        )}
      </div>
      {saved !== null && saved.trim() === "" && shared.trim() === "" && (
        <div className="flex flex-col gap-2 border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
          <p>{t("settings.json が空のため、Bash・Read などのツールは許可されていません(指示を実行しても、ツールを使えません)。拒否は、rm・sudo などの初期値だけを渡します。許可・拒否を書いた雛形を入れてから、保存してください。")}</p>
          <div>
            <Button size="sm" variant="outline" disabled={draft.trim() !== "" || template === null} onClick={() => {
                if (!template) return;
                setDraft(template);
                setEditing(true);
              }}>
              {t("許可・拒否の雛形を入れる")}
            </Button>
          </div>
        </div>
      )}
      <p className="text-xs text-muted-foreground">
        {own.trim() ? t("このエージェント専用の内容です。全体設定が変わっても更新されません。") : t("全体設定の内容を利用しています。全体設定が変わると、次の指示から自動で反映されます。編集して保存すると、このエージェント専用の個別設定になります。")}
      </p>
      {error && <p className="text-xs text-destructive">{error}</p>}
      <p className="text-xs text-muted-foreground">
        {t("Claude Code の settings.json の内容を貼り付けます(JSON のオブジェクト)。コンテナ内の Claude Code へ、指示ごとに --settings で渡します。モデルは model、許可するツールは permissions.allow、実行前に確認するツールは permissions.ask、拒否するツールは permissions.deny(Bash(git status:*) のような指定も可)で決めます。")}
      </p>
      <p className="text-xs text-muted-foreground">
        {t("コンテナ内の Claude Code のコマンドの拒否は、ここの permissions.deny で決めます(雛形に rm・sudo などの初期値が入っています)。消すとその保護は無くなります。env の値は、バックアップ・設定の書き出しにそのまま含まれます。コンテナの出力に値が出たときは伏せ字にします。")}
      </p>
    </div>
  );
}
