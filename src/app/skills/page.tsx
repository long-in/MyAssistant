"use client";

import { useEditShortcuts } from "@/lib/useEditShortcuts";
import { Kbd } from "@/components/Kbd";
import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { Download, Loader2, Pencil, Plus, Upload, Save, ScrollText, Trash2 } from "lucide-react";
import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { EmptyState, PaneHeader, StatusPill } from "@/components/GlobalPanels";
import {
  SKILL_LIMITS,
  SKILL_NAME_RE,
  type Skill,
  createSkill,
  deleteSkill,
  exportSkills,
  importSkills,
  listSkills,
  previewImportSkills,
  reportError,
  updateSkill,
} from "@/lib/api";

/** 編集中の内容。呼び出し語は入力欄では「,」区切りの1行で扱う。 */
type Draft = { name: string; description: string; triggers: string; body: string };

const EMPTY: Draft = { name: "", description: "", triggers: "", body: "" };

const toDraft = (s: Skill): Draft => ({ name: s.name, description: s.description, triggers: s.triggers.join(", "), body: s.body });

/** 「,」「、」区切りの入力を呼び出し語の配列にする(空と重複を除く)。 */
const parseTriggers = (text: string) => [...new Set(text.split(/[,、]/).map((x) => x.trim()).filter(Boolean))];

const toSkill = (d: Draft): Skill => ({
  name: d.name.trim(),
  description: d.description.trim(),
  triggers: parseTriggers(d.triggers),
  body: d.body.trim(),
});

export default function Skills() {
  const t = useT();
  const [skills, setSkills] = useState<Skill[]>([]);
  const [indexTokens, setIndexTokens] = useState(0);
  // 選択中のスキル名。新規作成中は`null`で`creating`が真
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [saved, setSaved] = useState<Draft>(EMPTY);
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [editMode, setEditMode] = useState(false);
  const [saving, setSaving] = useState(false);
  const [transferring, setTransferring] = useState(false);

  const refresh = useCallback(
    () =>
      listSkills()
        .then((o) => {
          setSkills(o.skills);
          setIndexTokens(o.index_tokens);
        })
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

  const select = async (skill: Skill) => {
    if (skill.name === selected && !creating) return;
    if (!(await confirmDiscard())) return;
    const d = toDraft(skill);
    setCreating(false);
    setSelected(skill.name);
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

  const exportJson = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const dest = await saveDialog({
      title: t("スキルのエクスポート"),
      defaultPath: `myassistant-skills-${stamp}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!dest) return;
    setTransferring(true);
    try {
      const n = await exportSkills(dest);
      toast.success(t("{n}件のスキルを書き出しました。", { n }));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setTransferring(false);
    }
  };

  // 同名のスキルがあるときは、上書きするか、飛ばすかを取り込み前に確認する
  const importJson = async () => {
    if (!(await confirmDiscard())) return;
    const src = await openDialog({
      title: t("スキルのインポート"),
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof src !== "string") return;
    setTransferring(true);
    try {
      const p = await previewImportSkills(src);
      let overwrite = false;
      if (p.conflicts.length > 0) {
        const shown = p.conflicts.slice(0, 5).join("、") + (p.conflicts.length > 5 ? t("、ほか{n}件", { n: p.conflicts.length - 5 }) : "");
        overwrite = await ask(
          t("同じ名前のスキルが{n}件あります({names})。上書きしますか?「飛ばす」を選ぶと、同名のスキルは取り込みません。", { n: p.conflicts.length, names: shown }),
          { title: t("インポートの確認"), kind: "warning", okLabel: t("上書きする"), cancelLabel: t("飛ばす") },
        );
      }
      const r = await importSkills(src, overwrite);
      close();
      await refresh();
      toast.success(t("{added}件を追加、{updated}件を上書き、{skipped}件を飛ばしました。", { added: r.added, updated: r.updated, skipped: r.skipped }));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setTransferring(false);
    }
  };

  const skill = toSkill(draft);
  const nameOk = creating ? skill.name.length > 0 && skill.name.length <= SKILL_LIMITS.name && SKILL_NAME_RE.test(skill.name) : true;
  const nameTaken = creating && skills.some((s) => s.name.toLowerCase() === skill.name.toLowerCase());
  const triggersOk =
    skill.triggers.length <= SKILL_LIMITS.triggers && skill.triggers.every((x) => [...x].length <= SKILL_LIMITS.trigger);
  const fieldsOk =
    skill.description.length > 0 &&
    [...skill.description].length <= SKILL_LIMITS.description &&
    !/[<>\n]/.test(skill.description) &&
    !skill.triggers.some((x) => /[<>]/.test(x)) &&
    triggersOk &&
    skill.body.length > 0 &&
    [...skill.body].length <= SKILL_LIMITS.body;
  const canSave = dirty && nameOk && !nameTaken && fieldsOk && !saving;

  const save = async () => {
    if (!dirty && !creating && editMode) {
      setEditMode(false); // 変更が無ければ保存せず閲覧に戻る
      return;
    }
    if (!canSave) return;
    setSaving(true);
    try {
      const done = creating ? await createSkill(skill) : await updateSkill(skill);
      const d = toDraft(done);
      setCreating(false);
      setSelected(done.name);
      setSaved(d);
      setDraft(d);
      await refresh();
      setEditMode(false);
      toast.success(t("保存しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!selected) return;
    const ok = await ask(t("スキル「{name}」を削除します。この操作は元に戻せません。続行しますか?", { name: selected }), {
      title: t("削除の確認"),
      kind: "warning",
      okLabel: t("削除する"),
      cancelLabel: t("キャンセル"),
    });
    if (!ok) return;
    try {
      await deleteSkill(selected);
      close();
      await refresh();
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  // `e`で編集、Cmd/Ctrl+Sで保存
  useEditShortcuts({ canEdit: open, editMode, onEdit: () => setEditMode(true), onSave: save });

  const count = (s: string) => [...s].length;

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r bg-background/40 backdrop-blur">
        <PaneHeader
          icon={ScrollText}
          title="Skills"
          chips={<StatusPill on={skills.length > 0}>{t("スキル {n}件", { n: skills.length })}</StatusPill>}
        />
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="hud-panel hud-accent m-3 px-4 py-3">
            <p className="hud-heading mb-2">Matching</p>
            <p className="text-xs text-muted-foreground">
              {skills.length === 0
                ? t("スキルがないので、選定は行われず、トークンも使いません。")
                : t("発話のたびに、全スキルの名前と説明(約{n}トークン)を送って選びます。説明は短く保つと節約できます。", { n: indexTokens })}
            </p>
          </div>
          <div className="py-1">
            {skills.map((s) => (
              <button
                key={s.name}
                type="button"
                onClick={() => select(s)}
                className={`block w-full px-4 py-2 text-left text-sm transition-colors hover:bg-primary/5 ${
                  s.name === selected && !creating ? "bg-primary/10" : ""
                }`}
              >
                <span className="block truncate font-medium">{s.name}</span>
                <span className="block truncate text-xs text-muted-foreground">{s.description || t("(説明なし)")}</span>
              </button>
            ))}
            {skills.length === 0 && (
              <p className="px-4 py-6 text-center text-sm text-muted-foreground">{t("スキルはまだありません。")}</p>
            )}
          </div>
        </div>
        <div className="border-t px-4 py-3">
          <Button size="sm" className="w-full" onClick={startCreate}>
            <Plus /> {t("新しいスキル")}
          </Button>
          <div className="mt-2 grid grid-cols-2 gap-2">
            <Button size="sm" variant="outline" disabled={transferring} onClick={importJson}>
              <Upload /> {t("インポート")}
            </Button>
            <Button size="sm" variant="outline" disabled={transferring || skills.length === 0} onClick={exportJson}>
              <Download /> {t("エクスポート")}
            </Button>
          </div>
          <p className="mt-2 text-xs text-muted-foreground">
            {t("会話から作ることもできます。「〜をスキルにして」のように「スキル」と言って頼んでください。")}
          </p>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("定義はナレッジの「Skills」フォルダの SKILL.md です。ナレッジ画面から直接編集もできます。")}
          </p>
        </div>
      </aside>

      <article className="flex min-w-0 flex-1 flex-col">
        {open ? (
          <>
            <div className="flex items-center justify-between gap-3 border-b px-4 py-2">
              <span className="flex min-w-0 items-center gap-2 text-sm">
                <ScrollText className="size-4 shrink-0 text-primary" />
                <span className="truncate">{creating ? t("新しいスキル") : selected}</span>
                {dirty && <StatusPill on>{t("未保存")}</StatusPill>}
              </span>
              <div className="flex shrink-0 gap-2">
                {!editMode && (
                  <Button size="sm" variant="outline" onClick={() => setEditMode(true)}>
                    <Pencil /> {t("編集")} <Kbd>E</Kbd>
                  </Button>
                )}
                <Button size="sm" disabled={!canSave} onClick={save}>
                  {saving ? <Loader2 className="animate-spin" /> : <Save />}
                  {t("保存")} <Kbd>⌘S</Kbd>
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
                          maxLength={SKILL_LIMITS.name}
                          placeholder="morning-report"
                          aria-invalid={draft.name !== "" && (!nameOk || nameTaken)}
                          onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                        />
                        <span className="mt-1 block text-xs text-muted-foreground">
                          {nameTaken
                            ? t("同じ名前のスキルがあります。")
                            : t("英数字・日本語・「_」「-」のみ、{max}文字まで。作成後は変更できません。", { max: SKILL_LIMITS.name })}
                        </span>
                      </label>
                    )}
                    <label className="block">
                      <span className="hud-label mb-1 block">
                        {t("説明")} ({count(draft.description)}/{SKILL_LIMITS.description})
                      </span>
                      <Input
                        value={draft.description}
                        placeholder={t("朝の報告をまとめる")}
                        aria-invalid={count(draft.description) > SKILL_LIMITS.description || /[<>\n]/.test(draft.description)}
                        onChange={(e) => setDraft({ ...draft, description: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("どんな依頼に使うかを書きます。発話のたびに全スキル分が選定へ送られるので、短くします。「<」「>」は使えません。")}
                      </span>
                    </label>
                    <label className="block">
                      <span className="hud-label mb-1 block">{t("呼び出し語(任意)")}</span>
                      <Input
                        value={draft.triggers}
                        placeholder={t("朝の報告, 日報")}
                        aria-invalid={!triggersOk}
                        onChange={(e) => setDraft({ ...draft, triggers: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("依頼でよく使う言い回しを「,」で区切って書きます({n}個まで、1つ{max}文字まで)。選定のヒントになります。", {
                          n: SKILL_LIMITS.triggers,
                          max: SKILL_LIMITS.trigger,
                        })}
                      </span>
                    </label>
                    <label className="block">
                      <span className="hud-label mb-1 block">
                        {t("手順")} ({count(draft.body)}/{SKILL_LIMITS.body})
                      </span>
                      <Textarea
                        className="min-h-72 font-mono text-sm leading-relaxed"
                        value={draft.body}
                        spellCheck={false}
                        aria-invalid={count(draft.body) > SKILL_LIMITS.body}
                        onChange={(e) => setDraft({ ...draft, body: e.target.value })}
                      />
                      <span className="mt-1 block text-xs text-muted-foreground">
                        {t("使うときだけ読み込まれる、対応の指示文です。ファイル作成やコマンド実行は、通常どおり承認が必要です。")}
                      </span>
                    </label>
                  </>
                ) : (
                  <>
                    <p className="text-sm">{draft.description}</p>
                    {skill.triggers.length > 0 && (
                      <div className="flex flex-wrap gap-1.5">
                        {skill.triggers.map((x) => (
                          <Badge key={x} variant="secondary">{x}</Badge>
                        ))}
                      </div>
                    )}
                    <div className="prose prose-sm max-w-none dark:prose-invert">
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>{draft.body}</ReactMarkdown>
                    </div>
                  </>
                )}
              </div>
            </div>
          </>
        ) : (
          <EmptyState icon={ScrollText}>{t("スキルを選択するか、新しく作成してください。")}</EmptyState>
        )}
      </article>
    </div>
  );
}
