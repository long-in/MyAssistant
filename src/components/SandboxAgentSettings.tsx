"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { FolderOpen } from "lucide-react";
import { useT } from "@/lib/i18n";
import {
  type SandboxLlm,
  type SandboxProvider,
  getSandboxLlm,
  getSandboxWorkspaceDir,
  openSandboxWorkspaceDir,
  reportError,
  setSandboxLlm,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

/** チェックボックスで選べるツール(Rust 側の `DEFAULT_ALLOWED_TOOLS` と同じ)。 */
const TOOL_OPTIONS = ["Bash", "Read", "Edit", "Write", "Glob", "Grep", "NotebookEdit", "WebFetch", "WebSearch", "Task", "TodoWrite", "Skill"];

/** 一覧にないツール(`Bash(git status:*)` などのパターン指定)の入力(改行・カンマ区切り)を一覧にする。 */
const parseTools = (text: string) =>
  text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);

/**
 * エージェントごとのサンドボックスの設定。
 * - `/workspace`にマウントするフォルダ(担当システムのソースコードなどを置く、エージェントごとに決まった場所)。読み書きでマウントされる。
 * - コンテナ内の Claude Code の接続先(Anthropic API / Bedrock)・モデル・1回の指示の上限時間・許可するツール。
 * API キーは sandbox.env に書く(ここでは扱わない)。
 */
export default function SandboxAgentSettings() {
  const t = useT();
  const [dir, setDir] = useState<string | null>(null);
  const [saved, setSaved] = useState<SandboxLlm | null>(null);
  const [provider, setProvider] = useState<SandboxProvider>("anthropic");
  const [model, setModel] = useState("");
  const [timeout, setTimeoutMins] = useState("30");
  const [checked, setChecked] = useState<string[]>([]);
  const [tools, setTools] = useState("");
  const [busy, setBusy] = useState(false);

  const apply = (c: SandboxLlm) => {
    setSaved(c);
    setProvider(c.provider);
    setModel(c.model);
    setTimeoutMins(String(c.timeout_mins));
    setChecked(c.allowed_tools.filter((x) => TOOL_OPTIONS.includes(x)));
    setTools(c.allowed_tools.filter((x) => !TOOL_OPTIONS.includes(x)).join("\n"));
  };

  useEffect(() => {
    getSandboxWorkspaceDir().then(setDir).catch((e) => toast.error(reportError(e)));
    getSandboxLlm().then(apply).catch((e) => toast.error(reportError(e)));
  }, []);

  const openDir = async () => {
    try {
      await openSandboxWorkspaceDir();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const draft = (): SandboxLlm => ({
    provider,
    model: model.trim(),
    timeout_mins: Number.parseInt(timeout, 10) || 30,
    allowed_tools: [...TOOL_OPTIONS.filter((x) => checked.includes(x)), ...parseTools(tools)],
  });
  const dirty =
    saved !== null &&
    (() => {
      const d = draft();
      return (
        d.provider !== saved.provider ||
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
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <span className="hud-label">/workspace</span>
        <div className="flex flex-wrap items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-mono text-xs" title={dir ?? undefined}>{dir ?? "…"}</span>
          <Button size="sm" variant="outline" onClick={() => void openDir()}>
            <FolderOpen className="size-3.5" />
            {t("Finder で開く")}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("このフォルダが、コンテナの /workspace に読み書きでマウントされます。担当システムのソースコードなどは、ここに置くか git clone してください。コンテナ内の作業でファイルが変更・削除されることがあります。")}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("エージェントを削除すると、このフォルダも削除されます。バックアップには含まれません。")}
        </p>
      </div>

      <div className="flex flex-col gap-2">
        <span className="hud-label">Claude Code</span>
        <div className="grid gap-2 sm:grid-cols-2">
          <label className="flex flex-col gap-1 text-xs">
            {t("接続先")}
            <Select value={provider} onValueChange={(v) => setProvider(v as SandboxProvider)}>
              <SelectTrigger size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="anthropic">Anthropic API</SelectItem>
                <SelectItem value="bedrock">Amazon Bedrock</SelectItem>
              </SelectContent>
            </Select>
          </label>
          <label className="flex flex-col gap-1 text-xs">
            {t("1回の指示の上限時間(分)")}
            <Input type="number" min={1} max={180} value={timeout} onChange={(e) => setTimeoutMins(e.target.value)} />
          </label>
          <label className="flex flex-col gap-1 text-xs sm:col-span-2">
            {t("モデル(空なら Claude Code の既定)")}
            <Input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={provider === "bedrock" ? "us.anthropic.claude-..." : "claude-..."}
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
          {provider === "bedrock"
            ? t("このエージェントに登録した AWS の設定から、一時キーを発行して渡します(MFA が必要なら、その都度入力を求めます)。")
            : t("API キーは、このエージェントの sandbox.env に ANTHROPIC_API_KEY として書いてください。")}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("拒否は「コマンド権限」の拒否の項目が使われます。コマンド実行や Web 検索を無効にしていると、対応するツールも使えません。")}
        </p>
        <div>
          <Button size="sm" disabled={!dirty || busy} onClick={() => void save()}>
            {t("保存")}
          </Button>
        </div>
      </div>
    </div>
  );
}
