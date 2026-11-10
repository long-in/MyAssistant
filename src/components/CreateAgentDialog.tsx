"use client";

import { useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, Loader2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { MAX_AGENTS, AGENT_NAME_MAX, createAgent, errorMessage } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { refreshWorkspace, switchTo, useWorkspace } from "@/lib/workspace";

/**
 * エージェントの作成。名称とイメージカラーだけを入力する。
 * リーダーが無い間と、上限(30件)のときは、作成できない理由を示す。作成後は、そのエージェントへ切り替える。
 */
export default function CreateAgentDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const router = useRouter();
  const { agents, palette, hasLeader } = useWorkspace();
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const full = agents.length >= MAX_AGENTS;

  useEffect(() => {
    inputRef.current?.focus();
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  // ほかのエージェントが使っている色は選べない。選んだ色が使えなければ、空いている先頭の色にする
  const usedBy = new Map(agents.map((p) => [p.color, p.name]));
  const color = picked && !usedBy.has(picked) ? picked : (palette.find((c) => !usedBy.has(c)) ?? "");
  const canSubmit = !busy && hasLeader && !full && name.trim().length > 0 && color !== "";

  const submit = async () => {
    setBusy(true);
    try {
      const created = await createAgent(name, color);
      await refreshWorkspace();
      await switchTo(created.id);
      toast.success(t("エージェントを作成しました。"), { description: t("設定を引き継ぐには、設定の「エージェント」タブにある「設定のコピー」を使ってください。") });
      onClose();
      router.push("/");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  // 呼び出し元の`backdrop-filter`の中では`fixed`が切り取られるため、bodyへ描画する
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <div role="dialog" aria-modal="true" aria-labelledby="create-agent-title" className="flex max-h-full w-full max-w-md flex-col rounded-xl border bg-card/80 p-5 text-sm shadow-2xl backdrop-blur-md">
        <h2 id="create-agent-title" className="text-base font-medium">
          {t("エージェントを作成")}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("名称とイメージカラーは、どのエージェントと話しているかを見分けるために表示されます。名称と色はエージェントごとに重複できず、「パーソナル」「ワーク」などは予約名です。({count}/{max}件)", { count: agents.length, max: MAX_AGENTS })}
        </p>
        <div className="mt-4 flex flex-col gap-4">
          <Input
            ref={inputRef}
            value={name}
            maxLength={AGENT_NAME_MAX}
            placeholder={t("エージェント名")}
            aria-label={t("エージェント名")}
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
          {!hasLeader && (
            <p className="text-xs text-destructive">
              {t("エージェントを作るには、先にリーダーを作成してください。")}{" "}
              <button
                type="button"
                className="underline"
                onClick={() => {
                  onClose();
                  router.push("/global#work");
                }}
              >
                {t("リーダーを作成する")}
              </button>
            </p>
          )}
          {full && <p className="text-xs text-destructive">{t("エージェントは30件までです。不要なエージェントを削除してください。")}</p>}
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="outline" onClick={onClose} disabled={busy}>
            {t("キャンセル")}
          </Button>
          <Button onClick={() => void submit()} disabled={!canSubmit}>
            {busy && <Loader2 className="animate-spin" />}
            {t("作成")}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
