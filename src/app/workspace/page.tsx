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
import { cn } from "@/lib/utils";
import { bumpSettingsVersion, refreshWorkspace, switchTo, useWorkspace } from "@/lib/workspace";

export default function WorkspacePage() {
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
        toast.success("Updated the project.");
        reset();
      } else {
        const created = await createProject(name, color);
        await refreshWorkspace();
        await switchTo(created.id); // 作ったらそのまま開く。APIキーが未登録なら設定画面へ案内される
        toast.success("Created the project.");
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
      `Delete the project "${p.name}"? Its conversations, settings, API keys and the knowledge in the default location are all deleted and this cannot be undone. A knowledge location you specified yourself is not deleted. Continue?`,
      { title: "Delete project", kind: "warning" },
    );
    if (!ok) return;
    try {
      await deleteProject(p.id);
      await refreshWorkspace();
      if (editing === p.id) reset();
      toast.success("Deleted the project.");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const targets = [
    { id: PERSONAL_ID, name: "Personal", color: null as string | null },
    ...projects.map((p) => ({ id: p.id, name: p.name, color: p.color as string | null })),
  ];
  const to = targets.find((x) => x.id === copyTo)?.id ?? active.id;
  const from = targets.find((x) => x.id === copyFrom && x.id !== to)?.id ?? targets.find((x) => x.id !== to)?.id ?? "";
  const nameOf = (id: string) => targets.find((x) => x.id === id)?.name ?? "";

  const copy = async () => {
    const ok = await ask(
      `Copy the settings of "${nameOf(from)}" to "${nameOf(to)}"? The same items in "${nameOf(to)}" are overwritten. ${withKeys ? "API keys are also saved in the destination. " : ""}Continue?`,
      { title: "Copy settings", kind: "warning" },
    );
    if (!ok) return;
    setBusy(true);
    try {
      const r = await copyWorkspaceSettings(from, to, withKeys, withProfile);
      if (to === active.id) bumpSettingsVersion();
      toast.success(`Copied ${r.settings} settings.` + (r.keys > 0 ? ` (${r.keys} API keys)` : ""));
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
          <h1 className="text-2xl font-semibold tracking-tight">Projects</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {"Each Work project is an independent agent. Conversations, knowledge, settings and API keys are not shared between projects or with Personal."}
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>{editing ? "Edit project" : "Create a project"}</CardTitle>
            <CardDescription>
              {`The name and image color are shown so you can tell which agent you are talking to. Each project needs its own name and color, and names like "Personal" and "Work" are reserved. (${projects.length}/${MAX_PROJECTS})`}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Input
              value={name}
              maxLength={PROJECT_NAME_MAX}
              placeholder="Project name"
              aria-label="Project name"
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && canSubmit && void submit()}
            />
            <div className="grid grid-cols-10 gap-2" role="radiogroup" aria-label="Image color">
              {palette.map((c) => {
                const owner = usedBy.get(c);
                return (
                  <button
                    key={c}
                    type="button"
                    role="radio"
                    aria-checked={color === c}
                    aria-label={owner ? `${c} (in use: ${owner})` : c}
                    title={owner ? `In use: ${owner}` : c}
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
                {editing ? "Save" : "Create"}
              </Button>
              {editing && (
                <Button variant="outline" onClick={reset}>
                  <X />
                  {"Cancel"}
                </Button>
              )}
              {full && !editing && (
                <span className="text-xs text-destructive">You can have up to 30 projects. Delete the ones you no longer need.</span>
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Copy settings</CardTitle>
            <CardDescription>
              {"Copies the settings of another Personal space or project as is. Models, voice, wake words, command permissions and similar settings are copied; knowledge, conversations and the pronunciation dictionary are not."}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2">
              <Select items={targets.filter((x) => x.id !== to).map((x) => ({ value: x.id, label: x.name }))} value={from} onValueChange={(v) => v && setCopyFrom(v as string)}>
                <SelectTrigger className="min-w-40" aria-label="Copy from">
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
                <SelectTrigger className="min-w-40" aria-label="Copy to">
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
                {"Also copy API keys"}
                <span className="block text-xs text-muted-foreground">Saves the Anthropic, AWS, Google and Aqua keys in the destination too (the same keys there are overwritten).</span>
              </span>
              <Switch checked={withKeys} onCheckedChange={setWithKeys} />
            </label>
            <label className="flex items-center justify-between gap-4 text-sm">
              <span>
                {"Also copy personal data"}
                <span className="block text-xs text-muted-foreground">Name, how to address you, birthday and so on. You normally should not copy these to a work project.</span>
              </span>
              <Switch checked={withProfile} onCheckedChange={setWithProfile} />
            </label>
            <div>
              <Button onClick={() => void copy()} disabled={busy || from === ""}>
                <Copy />
                {"Copy"}
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Project list</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col divide-y">
            {projects.length === 0 && <p className="py-3 text-sm text-muted-foreground">No projects yet.</p>}
            {projects.map((p) => {
              const current = active.id === p.id && active.id !== PERSONAL_ID;
              return (
                <div key={p.id} className="flex items-center justify-between gap-3 py-3">
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="size-4 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
                    <span className="truncate text-sm font-medium">{p.name}</span>
                    {current && <span className="shrink-0 text-xs text-muted-foreground">Selected</span>}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    {!current && (
                      <Button size="sm" variant="outline" onClick={() => void open(p.id)}>
                        {"Open"}
                      </Button>
                    )}
                    <Button size="icon" variant="ghost" title="Edit" aria-label="Edit" onClick={() => startEdit(p)}>
                      <Pencil />
                    </Button>
                    <Button size="icon" variant="ghost" title="Delete" aria-label="Delete" onClick={() => void remove(p)}>
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
