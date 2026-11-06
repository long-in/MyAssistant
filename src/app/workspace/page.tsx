"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { Check, Pencil, Plus, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  MAX_PROJECTS,
  PERSONAL_ID,
  PROJECT_NAME_MAX,
  createProject,
  deleteProject,
  errorMessage,
  updateProject,
  type Project,
} from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { refreshWorkspace, switchTo, useWorkspace } from "@/lib/workspace";

/** イメージカラーの候補。白・黒に近い色は、背景に溶けるため避ける。 */
const PALETTE = ["#ef4444", "#f97316", "#eab308", "#22c55e", "#14b8a6", "#06b6d4", "#3b82f6", "#8b5cf6", "#d946ef", "#ec4899"];

export default function WorkspacePage() {
  const t = useT();
  const router = useRouter();
  const { active, projects } = useWorkspace();
  const [editing, setEditing] = useState<string | null>(null); // 編集中のプロジェクトID。nullなら新規作成
  const [name, setName] = useState("");
  const [color, setColor] = useState(PALETTE[6]);
  const [busy, setBusy] = useState(false);
  const full = projects.length >= MAX_PROJECTS;

  const reset = () => {
    setEditing(null);
    setName("");
    setColor(PALETTE[6]);
  };

  const startEdit = (p: Project) => {
    setEditing(p.id);
    setName(p.name);
    setColor(p.color);
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
        router.push("/");
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

  const canSubmit = !busy && name.trim().length > 0 && (editing !== null || !full);

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
              {t("名称とイメージカラーは、どのエージェントと話しているかを見分けるために表示されます。({count}/{max}件)", {
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
            <div className="flex flex-wrap items-center gap-2" role="radiogroup" aria-label={t("イメージカラー")}>
              {PALETTE.map((c) => (
                <button
                  key={c}
                  type="button"
                  role="radio"
                  aria-checked={color === c}
                  aria-label={c}
                  onClick={() => setColor(c)}
                  className={cn(
                    "flex size-7 items-center justify-center rounded-full ring-offset-2 ring-offset-background transition",
                    color === c && "ring-2 ring-foreground",
                  )}
                  style={{ backgroundColor: c }}
                >
                  {color === c && <Check className="size-4 text-white drop-shadow" />}
                </button>
              ))}
              <label className="ml-2 flex items-center gap-2 text-xs text-muted-foreground">
                {t("カスタム")}
                <input
                  type="color"
                  value={color}
                  onChange={(e) => setColor(e.target.value)}
                  className="size-7 cursor-pointer rounded border bg-transparent p-0"
                  aria-label={t("カスタム")}
                />
              </label>
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
