"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { ArrowRight, Check, Copy, Pencil, Plus, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  MAX_PROJECTS,
  PERSONAL_ID,
  PROJECT_NAME_MAX,
  copyWorkspaceSettings,
  createProject,
  deleteProject,
  errorMessage,
  updateProject,
  type Project,
} from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { bumpSettingsVersion, refreshWorkspace, switchTo, useWorkspace } from "@/lib/workspace";

export default function WorkspacePage() {
  const t = useT();
  const router = useRouter();
  const { active, projects, palette } = useWorkspace();
  const [editing, setEditing] = useState<string | null>(null); // 編集中のプロジェクトID。nullなら新規作成
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // 設定のコピー。コピー先は、選んでいなければ選択中のワークスペース。コピー元は、選んでいなければコピー先以外の先頭
  const [copyFrom, setCopyFrom] = useState<string | null>(null);
  const [copyTo, setCopyTo] = useState<string | null>(null);
  const [withKeys, setWithKeys] = useState(false);
  const [withProfile, setWithProfile] = useState(false);
  const full = projects.length >= MAX_PROJECTS;
  // ほかのプロジェクトが使っている色は選べない(編集中は自分の色を除く)。選んだ色が使えなければ、空いている先頭の色にする
  const usedBy = new Map(projects.filter((p) => p.id !== editing).map((p) => [p.color, p.name]));
  const color = picked && !usedBy.has(picked) ? picked : (palette.find((c) => !usedBy.has(c)) ?? "");

  const reset = () => {
    setEditing(null);
    setName("");
    setPicked(null);
  };

  const startEdit = (p: Project) => {
    setEditing(p.id);
    setName(p.name);
    setPicked(p.color);
  };

  const submit = async () => {
    setBusy(true);
    try {
      if (editing) {
        await updateProject(editing, name, color);
        await refreshWorkspace();
        toast.success(t("プロジェクトを更新しました。"));
        reset();
      } else {
        const created = await createProject(name, color);
        await refreshWorkspace();
        await switchTo(created.id); // 作ったらそのまま開く。APIキーが未登録なら設定画面へ案内される
        toast.success(t("プロジェクトを作成しました。"));
        reset();
      }
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const open = (id: string) =>
    switchTo(id)
      .then(() => router.push("/"))
      .catch((e) => toast.error(errorMessage(e)));

  const remove = async (p: Project) => {
    const ok = await ask(
      t("プロジェクト「{name}」を削除します。会話・ナレッジ・設定・APIキーもすべて削除され、元に戻せません。続行しますか?", { name: p.name }),
      { title: t("プロジェクトの削除"), kind: "warning" },
    );
    if (!ok) return;
    try {
      await deleteProject(p.id);
      await refreshWorkspace();
      if (editing === p.id) reset();
      toast.success(t("プロジェクトを削除しました。"));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const targets = [
    { id: PERSONAL_ID, name: t("パーソナル"), color: null as string | null },
    ...projects.map((p) => ({ id: p.id, name: p.name, color: p.color as string | null })),
  ];
  const to = targets.find((x) => x.id === copyTo)?.id ?? active.id;
  const from = targets.find((x) => x.id === copyFrom && x.id !== to)?.id ?? targets.find((x) => x.id !== to)?.id ?? "";
  const nameOf = (id: string) => targets.find((x) => x.id === id)?.name ?? "";

  const copy = async () => {
    const ok = await ask(
      t("「{from}」の設定を「{to}」へコピーします。「{to}」の同じ項目は上書きされます。{extra}続行しますか?", {
        from: nameOf(from),
        to: nameOf(to),
        extra: withKeys ? t("APIキーもコピー先に保存されます。") : "",
      }),
      { title: t("設定のコピー"), kind: "warning" },
    );
    if (!ok) return;
    setBusy(true);
    try {
      const r = await copyWorkspaceSettings(from, to, withKeys, withProfile);
      if (to === active.id) bumpSettingsVersion();
      toast.success(t("設定を{count}件コピーしました。", { count: r.settings }) + (r.keys > 0 ? t("(APIキー{keys}件)", { keys: r.keys }) : ""));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const canSubmit = !busy && name.trim().length > 0 && color !== "" && (editing !== null || !full);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-5 px-6 py-10">
        <div>
          <p className="hud-label mb-1">{"// Work"}</p>
          <h1 className="text-2xl font-semibold tracking-tight">{t("プロジェクト")}</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("ワークのプロジェクトは、それぞれが独立したエージェントです。会話・ナレッジ・設定・APIキーは、プロジェクト同士でも、パーソナルとも共有されません。")}
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>{editing ? t("プロジェクトを編集") : t("プロジェクトを作成")}</CardTitle>
            <CardDescription>
              {t("名称とイメージカラーは、どのエージェントと話しているかを見分けるために表示されます。色はプロジェクトごとに重複できません。({count}/{max}件)", {
                count: projects.length,
                max: MAX_PROJECTS,
              })}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Input
              value={name}
              maxLength={PROJECT_NAME_MAX}
              placeholder={t("プロジェクト名")}
              aria-label={t("プロジェクト名")}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && canSubmit && void submit()}
            />
            <div className="grid grid-cols-10 gap-2" role="radiogroup" aria-label={t("イメージカラー")}>
              {palette.map((c) => {
                const owner = usedBy.get(c);
                return (
                  <button
                    key={c}
                    type="button"
                    role="radio"
                    aria-checked={color === c}
                    aria-label={owner ? t("{color}(使用中: {name})", { color: c, name: owner }) : c}
                    title={owner ? t("使用中: {name}", { name: owner }) : c}
                    disabled={owner !== undefined}
                    onClick={() => setPicked(c)}
                    className={cn(
                      "relative flex aspect-square w-full max-w-8 items-center justify-center rounded-full ring-offset-2 ring-offset-background transition",
                      color === c && "ring-2 ring-foreground",
                      owner !== undefined && "cursor-not-allowed opacity-25",
                    )}
                    style={{ backgroundColor: c }}
                  >
                    {color === c && <Check className="size-4 text-white drop-shadow" />}
                    {owner !== undefined && <X className="size-4 text-white drop-shadow" />}
                  </button>
                );
              })}
            </div>
            <div className="flex items-center gap-2">
              <Button onClick={() => void submit()} disabled={!canSubmit}>
                {editing ? <Check /> : <Plus />}
                {editing ? t("保存") : t("作成")}
              </Button>
              {editing && (
                <Button variant="outline" onClick={reset}>
                  <X />
                  {t("キャンセル")}
                </Button>
              )}
              {full && !editing && (
                <span className="text-xs text-destructive">{t("プロジェクトは30件までです。不要なプロジェクトを削除してください。")}</span>
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{t("設定のコピー")}</CardTitle>
            <CardDescription>
              {t("ほかのパーソナル・プロジェクトの設定を、そのまま写します。モデル・音声・起動ワード・コマンド権限などが対象で、ナレッジ・会話・読み上げ辞書は写しません。")}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2">
              <Select items={targets.filter((x) => x.id !== to).map((x) => ({ value: x.id, label: x.name }))} value={from} onValueChange={(v) => v && setCopyFrom(v as string)}>
                <SelectTrigger className="min-w-40" aria-label={t("コピー元")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {targets.filter((x) => x.id !== to).map((x) => (
                    <SelectItem key={x.id} value={x.id}>
                      {x.color && <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: x.color }} />}
                      {x.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <ArrowRight className="size-4 text-muted-foreground" />
              <Select items={targets.map((x) => ({ value: x.id, label: x.name }))} value={to} onValueChange={(v) => v && setCopyTo(v as string)}>
                <SelectTrigger className="min-w-40" aria-label={t("コピー先")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {targets.map((x) => (
                    <SelectItem key={x.id} value={x.id}>
                      {x.color && <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: x.color }} />}
                      {x.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <label className="flex items-center justify-between gap-4 text-sm">
              <span>
                {t("APIキーもコピーする")}
                <span className="block text-xs text-muted-foreground">{t("Anthropic・AWS・Google・Aquaのキーを、コピー先にも保存します(コピー先の同じキーは上書き)。")}</span>
              </span>
              <Switch checked={withKeys} onCheckedChange={setWithKeys} />
            </label>
            <label className="flex items-center justify-between gap-4 text-sm">
              <span>
                {t("パーソナルデータもコピーする")}
                <span className="block text-xs text-muted-foreground">{t("名前・呼ばれ方・誕生日など。仕事用のプロジェクトへは、通常コピーしないでください。")}</span>
              </span>
              <Switch checked={withProfile} onCheckedChange={setWithProfile} />
            </label>
            <div>
              <Button onClick={() => void copy()} disabled={busy || from === ""}>
                <Copy />
                {t("コピーする")}
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{t("プロジェクト一覧")}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col divide-y">
            {projects.length === 0 && <p className="py-3 text-sm text-muted-foreground">{t("まだプロジェクトがありません。")}</p>}
            {projects.map((p) => {
              const current = active.id === p.id && active.id !== PERSONAL_ID;
              return (
                <div key={p.id} className="flex items-center justify-between gap-3 py-3">
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="size-4 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
                    <span className="truncate text-sm font-medium">{p.name}</span>
                    {current && <span className="shrink-0 text-xs text-muted-foreground">{t("選択中")}</span>}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    {!current && (
                      <Button size="sm" variant="outline" onClick={() => void open(p.id)}>
                        {t("開く")}
                      </Button>
                    )}
                    <Button size="icon" variant="ghost" title={t("編集")} aria-label={t("編集")} onClick={() => startEdit(p)}>
                      <Pencil />
                    </Button>
                    <Button size="icon" variant="ghost" title={t("削除")} aria-label={t("削除")} onClick={() => void remove(p)}>
                      <Trash2 />
                    </Button>
                  </div>
                </div>
              );
            })}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
