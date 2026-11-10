"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useT } from "@/lib/i18n";
import { type SandboxLlm, getSandboxLlm, reportError, setSandboxLlm } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Textarea } from "@/components/ui/textarea";
import { Heading } from "@/components/GlobalPanels";

/** チェックボックスで選べるツール(Rust 側の `DEFAULT_ALLOWED_TOOLS` と同じ)。 */
const TOOL_OPTIONS = ["Bash", "Read", "Edit", "Write", "Glob", "Grep", "NotebookEdit", "WebFetch", "WebSearch", "Task", "TodoWrite", "Skill"];

/** 一覧にないツール(`Bash(git status:*)` などのパターン指定)の入力(改行・カンマ区切り)を一覧にする。 */
const parseTools = (text: string) =>
  text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);

/**
 * ワークのエージェントの LLM。専用のDockerコンテナの中の Claude Code の接続先(Amazon Bedrock 固定)・モデル・
 * 1回の指示の上限時間・許可するツールを設定する(LLM タブに置く)。
 * 認証は、AI Link に登録した AWS の設定から発行した一時キーが、指示ごとの環境変数でコンテナへ渡される(sandbox.env の同名が優先)。
 */
export default function ClaudeCodeSettings() {
  const t = useT();
  const [saved, setSaved] = useState<SandboxLlm | null>(null);
  const [model, setModel] = useState("");
  const [timeout, setTimeoutMins] = useState("30");
  const [checked, setChecked] = useState<string[]>([]);
  const [tools, setTools] = useState("");
  const [busy, setBusy] = useState(false);

  const apply = (c: SandboxLlm) => {
    setSaved(c);
    setModel(c.model);
    setTimeoutMins(String(c.timeout_mins));
    setChecked(c.allowed_tools.filter((x) => TOOL_OPTIONS.includes(x)));
    setTools(c.allowed_tools.filter((x) => !TOOL_OPTIONS.includes(x)).join("\n"));
  };

  useEffect(() => {
    getSandboxLlm().then(apply).catch((e) => toast.error(reportError(e)));
  }, []);

  const draft = (): SandboxLlm => ({
    provider: "bedrock",
    model: model.trim(),
    timeout_mins: Number.parseInt(timeout, 10) || 30,
    allowed_tools: [...TOOL_OPTIONS.filter((x) => checked.includes(x)), ...parseTools(tools)],
  });
  const dirty =
    saved !== null &&
    (() => {
      const d = draft();
      return (
        d.model !== saved.model ||
        d.timeout_mins !== saved.timeout_mins ||
        d.allowed_tools.join("\n") !== saved.allowed_tools.join("\n")
      );
    })();

  const save = async () => {
    setBusy(true);
    try {
      apply(await setSandboxLlm(draft()));
      toast.success(t("保存しました。次の指示から反映されます。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent flex flex-col gap-3 px-5 py-4">
      <Heading title="Claude Code" help={t("コンテナ内のAI")}>
        <p>{t("このエージェントへの指示は、専用のDockerコンテナの中の Claude Code が受け持ちます。ここで、その接続先・モデル・上限時間・許可するツールを設定します。")}</p>
        <ul>
          <li>{t("認証は、下の AI Link に登録した AWS の設定から発行した一時キーが、指示ごとの環境変数でコンテナへ渡されます。ファイルとしてはコンテナに置きません。")}</li>
          <li>{t("別のキーを使いたいときは、Sandbox タブの sandbox.env に同じ名前で書くと、そちらが優先されます。")}</li>
          <li>{t("下の AI Settings のモデル選択や AI Limits の一部は、チャットでは使われません(ナレッジ整理などホスト側の処理でだけ使われます)。")}</li>
        </ul>
      </Heading>
      <div className="grid gap-2 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-xs">
          {t("1回の指示の上限時間(分)")}
          <Input type="number" min={1} max={180} value={timeout} onChange={(e) => setTimeoutMins(e.target.value)} />
        </label>
        <label className="flex flex-col gap-1 text-xs sm:col-span-2">
          {t("モデル(空なら Claude Code の既定)")}
          <Input
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="us.anthropic.claude-..."
            className="font-mono"
            spellCheck={false}
          />
        </label>
        <div className="flex flex-col gap-2 text-xs sm:col-span-2">
          {t("許可するツール")}
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            {TOOL_OPTIONS.map((name) => (
              <label key={name} className="flex items-center gap-2 font-mono">
                <Checkbox
                  checked={checked.includes(name)}
                  onCheckedChange={(v) => setChecked((cur) => (v ? [...cur, name] : cur.filter((x) => x !== name)))}
                />
                {name}
              </label>
            ))}
          </div>
        </div>
        <label className="flex flex-col gap-1 text-xs sm:col-span-2">
          {t("その他の許可(1行に1つ。Bash(git status:*) のような指定)")}
          <Textarea value={tools} onChange={(e) => setTools(e.target.value)} className="min-h-16 font-mono text-xs" spellCheck={false} />
        </label>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("このエージェントに登録した AWS の設定から、一時キーを発行して渡します(MFA が必要なら、その都度入力を求めます)。")}
      </p>
      <p className="text-xs text-muted-foreground">
        {t("拒否は「コマンド権限」の拒否の項目が使われます。コマンド実行や Web 検索を無効にしていると、対応するツールも使えません。")}
      </p>
      <div>
        <Button size="sm" disabled={!dirty || busy} onClick={() => void save()}>
          {t("保存")}
        </Button>
      </div>
    </section>
  );
}
