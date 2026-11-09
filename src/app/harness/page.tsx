"use client";

import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Eye, Loader2, Pencil, Plus, Save, Trash2, Workflow } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { EmptyState, PaneHeader, StatusPill } from "@/components/GlobalPanels";
import {
  HARNESS_LIMITS,
  HARNESS_TOOLS,
  SKILL_NAME_RE,
  type Harness,
  type HarnessOverview,
  createHarness,
  deleteHarness,
  listHarnesses,
  reportError,
  updateHarness,
} from "@/lib/api";

/** 編集中の内容。呼び出し語は「,」区切りの1行、参照範囲は1行1件、最大ステップ数は空なら既定として扱う。 */
type Draft = {
  name: string;
  description: string;
  triggers: string;
  paths: string;
  tools: string[];
  maxSteps: string;
  output: string;
  body: string;
};

const EMPTY: Draft = { name: "", description: "", triggers: "", paths: "", tools: [], maxSteps: "", output: "", body: "" };

const TOOL_LABELS: Record<(typeof HARNESS_TOOLS)[number], string> = {
  list_files: "ファイルの一覧",
  read_file: "ファイルの読み取り",
  search_files: "ファイルの検索",
  remember: "覚える",
  forget: "忘れる",
  create_file: "ファイルの作成",
  propose_plan: "作業計画の提示",
  run_command: "コマンドの実行",
  web_search: "Web検索",
  web_fetch: "ページの取得",
};

const toDraft = (h: Harness): Draft => ({
  name: h.name,
  description: h.description,
  triggers: h.triggers.join(", "),
  paths: h.paths.join("\n"),
  tools: h.tools,
  maxSteps: h.max_steps === null ? "" : String(h.max_steps),
  output: h.output,
  body: h.body,
});

/** 「,」「、」区切りの入力を配列にする(空と重複を除く)。 */
const parseList = (text: string, sep: RegExp) => [...new Set(text.split(sep).map((x) => x.trim()).filter(Boolean))];

/** 参照範囲は、先頭の「./」と末尾の「/」を取り除く。 */
const parsePaths = (text: string) => [...new Set(parseList(text, /\n/).map((p) => p.replace(/^\.\//, "").replace(/\/+$/, "")).filter(Boolean))];

const parseSteps = (text: string): number | null => {
  const n = Number(text.trim());
  return text.trim() === "" || !Number.isInteger(n) ? null : n;
};

const toHarness = (d: Draft): Harness => ({
  name: d.name.trim(),
  description: d.description.trim(),
  triggers: parseList(d.triggers, /[,、]/),
  paths: parsePaths(d.paths),
  tools: d.tools,
  max_steps: parseSteps(d.maxSteps),
  output: d.output.trim(),
  body: d.body.trim(),
});

const count = (s: string) => [...s].length;

/** 参照範囲として使えるか(Rust側の検証と同じ条件: 相対パスで、「..」と「.」始まりの要素は不可)。 */
const pathOk = (p: string) =>
  count(p) <= HARNESS_LIMITS.path && !p.startsWith("/") && !p.includes("\\") && p.split("/").every((x) => x !== "" && !x.startsWith("."));

export default function HarnessPage() {
  const t = useT();
  const [overview, setOverview] = useState<HarnessOverview>({ harnesses: [], broken: [] });
  // 選択中のハーネス名。新規作成中は`null`で`creating`が真
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [saved, setSaved] = useState<Draft>(EMPTY);
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [editMode, setEditMode] = useState(false);
  const [saving, setSaving] = useState(false);
  const { harnesses, broken } = overview;

  const refresh = useCallback(
    () =>
      listHarnesses()
        .then(setOverview)
        .catch((e) => toast.error(reportError(e))),
    [],
  );

  useEffect(() => {
    refresh();
  }, [refresh]);

  const open = selected !== null || creating;
  const dirty = open && JSON.stringify(draft) !== JSON.stringify(saved);

  /** 未保存の編集があれば破棄してよいか確認する。 */
  const confirmDiscard = async () =>
    !dirty ||
    (await ask(t("保存していない変更があります。破棄しますか?"), {
      title: t("未保存の変更"),
      kind: "warning",
      okLabel: t("破棄する"),
      cancelLabel: t("キャンセル"),
    }));

  const select = async (h: Harness) => {
    if (h.name === selected && !creating) return;
    if (!(await confirmDiscard())) return;
    const d = toDraft(h);
    setCreating(false);
    setSelected(h.name);
    setSaved(d);
    setDraft(d);
    setEditMode(false);
  };

  const startCreate = async () => {
    if (!(await confirmDiscard())) return;
    setCreating(true);
    setSelected(null);
    setSaved(EMPTY);
    setDraft(EMPTY);
    setEditMode(true);
  };

  const close = () => {
    setCreating(false);
    setSelected(null);
    setSaved(EMPTY);
    setDraft(EMPTY);
    setEditMode(false);
  };

  const h = toHarness(draft);
  const nameOk = creating ? h.name.length > 0 && count(h.name) <= HARNESS_LIMITS.name && SKILL_NAME_RE.test(h.name) : true;
  const nameTaken = creating && harnesses.some((x) => x.name.toLowerCase() === h.name.toLowerCase());
  const triggersOk = h.triggers.length <= HARNESS_LIMITS.triggers && h.triggers.every((x) => count(x) <= HARNESS_LIMITS.trigger && !/[<>]/.test(x));
  const pathsOk = h.paths.length <= HARNESS_LIMITS.paths && h.paths.every(pathOk);
  const stepsText = draft.maxSteps.trim();
  const stepsOk = stepsText === "" || (h.max_steps !== null && h.max_steps >= HARNESS_LIMITS.maxSteps[0] && h.max_steps <= HARNESS_LIMITS.maxSteps[1]);
  const fieldsOk =
    h.description.length > 0 &&
    count(h.description) <= HARNESS_LIMITS.description &&
    !/[<>\n]/.test(h.description) &&
    triggersOk &&
    pathsOk &&
    stepsOk &&
    count(h.output) <= HARNESS_LIMITS.output &&
    !h.output.includes("\n") &&
    h.body.length > 0 &&
    count(h.body) <= HARNESS_LIMITS.body;
  const canSave = dirty && nameOk && !nameTaken && fieldsOk && !saving;

  const save = async () => {
    if (!canSave) return;
    setSaving(true);
    try {
      const done = creating ? await createHarness(h) : await updateHarness(h);
      const d = toDraft(done);
      setCreating(false);
      setSelected(done.name);
      setSaved(d);
      setDraft(d);
      await refresh();
      toast.success(t("保存しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!selected) return;
    const ok = await ask(t("ハーネス「{name}」を削除します。この操作は元に戻せません。続行しますか?", { name: selected }), {
      title: t("削除の確認"),
      kind: "warning",
      okLabel: t("削除する"),
      cancelLabel: t("キャンセル"),
    });
    if (!ok) return;
    try {
      await deleteHarness(selected);
      close();
      await refresh();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  // Cmd/Ctrl+Sで保存
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const toggleTool = (tool: string) =>
    setDraft({ ...draft, tools: draft.tools.includes(tool) ? draft.tools.filter((x) => x !== tool) : [...draft.tools, tool] });

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <PaneHeader
          icon={Workflow}
          title="Harness"
          chips={<StatusPill on={harnesses.length > 0}>{t("ハーネス {n}件", { n: harnesses.length })}</StatusPill>}
        />
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="hud-panel hud-accent m-3 px-4 py-3">
            <p className="hud-label mb-2">Harness</p>
            <p className="text-xs text-muted-foreground">
              {t("タスクごとに、参照するナレッジの範囲・使えるツール・進め方を決めておきます。チャットで選ぶか、指定しなければ発話に合うものが自動で選ばれます。")}
            </p>
          </div>
          <div className="py-1">
            {harnesses.map((x) => (
              <button
                key={x.name}
                type="button"
                onClick={() => select(x)}
                className={`block w-full px-4 py-2 text-left text-sm transition-colors hover:bg-primary/5 ${
                  x.name === selected && !creating ? "bg-primary/10" : ""
                }`}
              >
                <span className="block truncate font-medium">{x.name}</span>
                <span className="block truncate text-xs text-muted-foreground">{x.description || t("(説明なし)")}</span>
              </button>
            ))}
            {harnesses.length === 0 && broken.length === 0 && (
              <p className="px-4 py-6 text-center text-sm text-muted-foreground">{t("ハーネスはまだありません。")}</p>
            )}
          </div>
          {broken.length > 0 && (
            <div className="border-t py-2">
              <p className="hud-label px-4 py-1">{t("読み込めなかったファイル")}</p>
              {broken.map((b) => (
                <div key={b.name} className="px-4 py-2 text-xs">
                  <span className="flex items-center gap-1 font-medium text-destructive">
                    <AlertTriangle className="size-3.5 shrink-0" />
                    <span className="truncate">Harness/{b.name}.md</span>
                  </span>
                  <span className="mt-0.5 block text-muted-foreground">{b.error}</span>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="border-t px-4 py-3">
          <Button size="sm" className="w-full" onClick={startCreate}>
            <Plus /> {t("新しいハーネス")}
          </Button>
          <p className="mt-2 text-xs text-muted-foreground">
            {t("定義はナレッジの「Harness」フォルダのMarkdownです。ナレッジ画面から直接編集もできます。")}
          </p>
        </div>
      </aside>

      <article className="flex min-w-0 flex-1 flex-col">
        {open ? (
          <>
            <div className="flex items-center justify-between gap-3 border-b px-4 py-2">
              <span className="flex min-w-0 items-center gap-2 text-sm">
                <Workflow className="size-4 shrink-0 text-primary" />
                <span className="truncate">{creating ? t("新しいハーネス") : selected}</span>
                {dirty && <StatusPill on>{t("未保存")}</StatusPill>}
              </span>
              <div className="flex shrink-0 gap-2">
                <Button size="sm" variant="outline" onClick={() => setEditMode((v) => !v)}>
                  {editMode ? <Eye /> : <Pencil />}
                  {editMode ? t("プレビュー") : t("編集")}
                </Button>
                <Button size="sm" disabled={!canSave} onClick={save}>
                  {saving ? <Loader2 className="animate-spin" /> : <Save />}
                  {t("保存")}
                </Button>
                {!creating && (
                  <Button size="sm" variant="ghost" className="text-destructive" onClick={remove}>
                    <Trash2 /> {t("削除")}
                  </Button>
                )}
              </div>
            </div>
            <div className="flex-1 overflow-y-auto px-10 py-6">
              <div className="mx-auto max-w-3xl space-y-5">
                {editMode ? (
                  <>
                    {creating && (
                      <label className="block">
                        <span className="hud-label mb-1 block">{t("名前")}</span>
                        <Input
                          value={draft.name}
                          maxLength={HARNESS_LIMITS.name}
                          placeholder="weekly-report"
                          aria-invalid={draft.name !== "" && (!nameOk || nameTaken)}
                          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                        />
                        <span className="mt-1 block text-xs text-muted-foreground">
                          {nameTaken
                            ? t("同じ名前のハーネスがあります。")
                            : t("英数字・日本語・「_」「-」のみ、{max}文字まで。作成後は変更できません。", { max: HARNESS_LIMITS.name })}
                        </span>
                      </label>
                    )}
                    <label className="block">
                      <span className="hud-label mb-1 block">
                        {t("説明")} ({count(draft.description)}/{HARNESS_LIMITS.description})
                      </span>
                      <Input
                        value={draft.description}
                        placeholder={t("週報を作る")}
                        aria-invalid={count(draft.description) > HARNESS_LIMITS.description || /[<>\n]/.test(draft.description)}
                        onChange={(e) => setDraft({ ...draft, description: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("どんな依頼に使うかを書きます。自動選定のたびに全ハーネス分が送られるので、短くします。「<」「>」は使えません。")}
                      </span>
                    </label>
                    <label className="block">
                      <span className="hud-label mb-1 block">{t("呼び出し語(任意)")}</span>
                      <Input
                        value={draft.triggers}
                        placeholder={t("週報, 今週のまとめ")}
                        aria-invalid={!triggersOk}
                        onChange={(e) => setDraft({ ...draft, triggers: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("自動選定のヒントになる言い回しを「,」で区切って書きます({n}個まで、1つ{max}文字まで)。", {
                          n: HARNESS_LIMITS.triggers,
                          max: HARNESS_LIMITS.trigger,
                        })}
                      </span>
                    </label>
                    <label className="block">
                      <span className="hud-label mb-1 block">{t("参照範囲(任意)")}</span>
                      <Textarea
                        className="min-h-20 font-mono text-sm"
                        value={draft.paths}
                        placeholder={"Projects\nMemory"}
                        spellCheck={false}
                        aria-invalid={!pathsOk}
                        onChange={(e) => setDraft({ ...draft, paths: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("読んでよいナレッジのフォルダを、ナレッジ直下からの相対パスで1行に1件書きます({n}件まで)。空なら制限しません。", {
                          n: HARNESS_LIMITS.paths,
                        })}
                      </span>
                    </label>
                    <fieldset>
                      <legend className="hud-label mb-1">{t("使えるツール(任意)")}</legend>
                      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 sm:grid-cols-3">
                        {HARNESS_TOOLS.map((tool) => (
                          <label key={tool} className="flex items-center gap-2 text-sm">
                            <input type="checkbox" checked={draft.tools.includes(tool)} onChange={() => toggleTool(tool)} />
                            <span>{t(TOOL_LABELS[tool])}</span>
                          </label>
                        ))}
                      </div>
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("何も選ばなければ、通常どおり全てのツールを使えます。選ぶと、選んだものだけを使えます(スキルは使えなくなります)。コマンド実行・Web検索は、設定で有効にしている場合だけ使えます。")}
                      </span>
                    </fieldset>
                    <label className="block">
                      <span className="hud-label mb-1 block">{t("最大ステップ数(任意)")}</span>
                      <Input
                        value={draft.maxSteps}
                        inputMode="numeric"
                        className="w-32"
                        placeholder={t("既定")}
                        aria-invalid={!stepsOk}
                        onChange={(e) => setDraft({ ...draft, maxSteps: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("1回の依頼でAIを呼ぶ回数の上限({min}〜{max})。既定の上限(20回、コマンド実行が有効なら40回)を超えて増やすことはできません。", {
                          min: HARNESS_LIMITS.maxSteps[0],
                          max: HARNESS_LIMITS.maxSteps[1],
                        })}
                      </span>
                    </label>
                    <label className="block">
                      <span className="hud-label mb-1 block">
                        {t("出力形式(任意)")} ({count(draft.output)}/{HARNESS_LIMITS.output})
                      </span>
                      <Input
                        value={draft.output}
                        placeholder={t("箇条書きで、見出しは3つまで")}
                        aria-invalid={count(draft.output) > HARNESS_LIMITS.output}
                        onChange={(e) => setDraft({ ...draft, output: e.target.value })}
                      />
                    </label>
                    <label className="block">
                      <span className="hud-label mb-1 block">
                        {t("進め方")} ({count(draft.body)}/{HARNESS_LIMITS.body})
                      </span>
                      <Textarea
                        className="min-h-72 font-mono text-sm leading-relaxed"
                        value={draft.body}
                        spellCheck={false}
                        aria-invalid={count(draft.body) > HARNESS_LIMITS.body}
                        onChange={(e) => setDraft({ ...draft, body: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("このハーネスで依頼を進めるときの指示文です。ファイル作成やコマンド実行は、通常どおり承認が必要です。")}
                      </span>
                    </label>
                  </>
                ) : (
                  <>
                    <p className="text-sm">{draft.description}</p>
                    {h.triggers.length > 0 && (
                      <div className="flex flex-wrap gap-1.5">
                        {h.triggers.map((x) => (
                          <Badge key={x} variant="secondary">{x}</Badge>
                        ))}
                      </div>
                    )}
                    <dl className="grid grid-cols-[8rem_1fr] gap-x-4 gap-y-2 text-sm">
                      <dt className="hud-label">{t("参照範囲")}</dt>
                      <dd>{h.paths.length > 0 ? h.paths.join(", ") : t("制限なし")}</dd>
                      <dt className="hud-label">{t("使えるツール")}</dt>
                      <dd>{h.tools.length > 0 ? h.tools.map((x) => t(TOOL_LABELS[x as keyof typeof TOOL_LABELS] ?? x)).join(", ") : t("制限なし")}</dd>
                      <dt className="hud-label">{t("最大ステップ数")}</dt>
                      <dd>{h.max_steps ?? t("既定")}</dd>
                      {h.output && (
                        <>
                          <dt className="hud-label">{t("出力形式")}</dt>
                          <dd>{h.output}</dd>
                        </>
                      )}
                    </dl>
                    <div className="prose prose-sm max-w-none dark:prose-invert">
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>{draft.body}</ReactMarkdown>
                    </div>
                  </>
                )}
              </div>
            </div>
          </>
        ) : (
          <EmptyState icon={Workflow}>{t("ハーネスを選択するか、新しく作成してください。")}</EmptyState>
        )}
      </article>
    </div>
  );
}
