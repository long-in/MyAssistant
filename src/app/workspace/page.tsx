"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { Check, Pencil, Plus, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import CopyConfirmModal from "@/components/CopyConfirmModal";
import CopyOptions from "@/components/CopyOptions";
import CopySettingsCard from "@/components/CopySettingsCard";
import SpaceChip from "@/components/SpaceChip";
import {
  MAX_SPACES,
  PERSONAL_ID,
  SPACE_NAME_MAX,
  createSpace,
  deleteSpace,
  errorMessage,
  updateSpace,
  type Space,
} from "@/lib/api";
import { type CopyRequest, copyTargets } from "@/lib/copyTargets";
import { cn } from "@/lib/utils";
import { bumpSettingsVersion, refreshWorkspace, switchTo, useWorkspace } from "@/lib/workspace";

/** 作成フォームで、設定をコピーしないことを表す選択肢の値。 */
const NO_COPY = "none";

export default function WorkspacePage() {
  const router = useRouter();
  const { active, spaces, palette } = useWorkspace();
  const [editing, setEditing] = useState<string | null>(null); // 編集中のスペースID。nullなら新規作成
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // 作成と同時に行う設定のコピー(コピー元。コピーしないときは`NO_COPY`)
  const [createFrom, setCreateFrom] = useState(NO_COPY);
  const [createKeys, setCreateKeys] = useState(false);
  const [createProfile, setCreateProfile] = useState(false);
  // 最終確認を待っているコピー。確認のモーダルは、このページで1つだけ出す
  const [copyRequest, setCopyRequest] = useState<CopyRequest | null>(null);
  // 作ったばかりのスペース。設定のコピー先の候補にする
  const [justCreated, setJustCreated] = useState<string | null>(null);
  const full = spaces.length >= MAX_SPACES;
  // ほかのスペースが使っている色は選べない(編集中は自分の色を除く)。選んだ色が使えなければ、空いている先頭の色にする
  const usedBy = new Map(spaces.filter((p) => p.id !== editing).map((p) => [p.color, p.name]));
  const color = picked && !usedBy.has(picked) ? picked : (palette.find((c) => !usedBy.has(c)) ?? "");

  const reset = () => {
    setEditing(null);
    setName("");
    setPicked(null);
    setCreateFrom(NO_COPY);
    setCreateKeys(false);
    setCreateProfile(false);
  };

  const startEdit = (p: Space) => {
    setEditing(p.id);
    setName(p.name);
    setPicked(p.color);
  };

  const submit = async () => {
    setBusy(true);
    try {
      if (editing) {
        await updateSpace(editing, name, color);
        await refreshWorkspace();
        toast.success("Updated the space.");
        reset();
      } else {
        const created = await createSpace(name, color);
        await refreshWorkspace();
        // 作成後は切り替えない(画面が作り直されて、入力中の内容や選択が失われるため)。開くのは、一覧の「Open」から
        setJustCreated(created.id);
        if (createFrom !== NO_COPY) {
          // 設定のコピーを選んでいたら、作成のあとに最終確認を出す(取りやめても、スペースは作成済みのまま)
          setCopyRequest({ from: createFrom, to: created.id, includeKeys: createKeys, includeProfile: createProfile });
          toast.success("Created the space.");
        } else {
          toast.success("Created the space.", { description: "To bring over settings, use “Copy settings” below." });
        }
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

  const remove = async (p: Space) => {
    const ok = await ask(
      `Delete the space "${p.name}"? Its conversations, settings, API keys and the knowledge in the default location are all deleted and this cannot be undone. A knowledge location you specified yourself is not deleted. Continue?`,
      { title: "Delete space", kind: "warning" },
    );
    if (!ok) return;
    try {
      await deleteSpace(p.id);
      await refreshWorkspace();
      if (editing === p.id) reset();
      toast.success("Deleted the space.");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const targets = copyTargets(spaces);

  const copied = (r: { settings: number; keys: number }, req: CopyRequest) => {
    toast.success(`Copied ${r.settings} settings.` + (r.keys > 0 ? ` (${r.keys} API keys)` : ""));
    if (req.to === active.id) bumpSettingsVersion(); // 選択中のスペースへコピーしたら、設定画面などを読み直す
  };

  const canSubmit = !busy && name.trim().length > 0 && color !== "" && (editing !== null || !full);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-5 px-6 py-10">
        <div>
          <p className="hud-label mb-1">{"// Work"}</p>
          <h1 className="text-2xl font-semibold tracking-tight">Spaces</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {"Each Work space is an independent agent. Conversations, knowledge, settings and API keys are not shared between spaces or with Personal."}
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>{editing ? "Edit space" : "Create a space"}</CardTitle>
            <CardDescription>
              {`The name and image color are shown so you can tell which agent you are talking to. Each space needs its own name and color, and names like "Personal" and "Work" are reserved. (${spaces.length}/${MAX_SPACES})`}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Input
              value={name}
              maxLength={SPACE_NAME_MAX}
              placeholder="Space name"
              aria-label="Space name"
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
            {!editing && (
              <div className="flex flex-col gap-3 rounded-lg border bg-muted/20 p-3">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="text-muted-foreground">Copy settings from</span>
                  <Select
                    items={[{ value: NO_COPY, label: "Don't copy" }, ...targets.map((t) => ({ value: t.id, label: t.name }))]}
                    value={createFrom}
                    onValueChange={(v) => v && setCreateFrom(v as string)}
                  >
                    <SelectTrigger className="min-w-40" aria-label="Copy settings from (optional)">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={NO_COPY}>Don&apos;t copy</SelectItem>
                      {targets.map((t) => (
                        <SelectItem key={t.id} value={t.id}>
                          <SpaceChip name={t.name} color={t.color} />
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                {createFrom !== NO_COPY && (
                  <>
                    <CopyOptions includeKeys={createKeys} includeProfile={createProfile} onKeys={setCreateKeys} onProfile={setCreateProfile} />
                    <p className="text-xs text-muted-foreground">You confirm once more after the space is created.</p>
                  </>
                )}
              </div>
            )}
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
                <span className="text-xs text-destructive">You can have up to 30 spaces. Delete the ones you no longer need.</span>
              )}
            </div>
          </CardContent>
        </Card>

        <CopySettingsCard targets={targets} suggestedTo={justCreated} onRequest={setCopyRequest} />

        <Card>
          <CardHeader>
            <CardTitle>Space list</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col divide-y">
            {spaces.length === 0 && <p className="py-3 text-sm text-muted-foreground">No spaces yet.</p>}
            {spaces.map((p) => {
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
      {copyRequest && (
        <CopyConfirmModal request={copyRequest} targets={targets} onClose={() => setCopyRequest(null)} onCopied={copied} />
      )}
    </div>
  );
}
