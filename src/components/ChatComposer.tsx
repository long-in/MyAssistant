"use client";

import { useT } from "@/lib/i18n";
import { Ear, Loader2, Mic, Send, Square, Volume2, VolumeX } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import ModelPicker from "./ModelPicker";
import { cn } from "@/lib/utils";
import type { KeyStatus, ModelInfo, Route } from "@/lib/api";

export type MicState = "idle" | "starting" | "recording" | "transcribing" | "standby";

type Props = {
  /** 呼びかけで起動する常時待機。 */
  standby: { on: boolean; available: boolean; onToggle: () => void };
  /** 音声入力。`hint`は押している間だけ録音するキーの名前。 */
  mic: { state: MicState; disabled: boolean; hint: string; onToggle: () => void };
  /** 返答の読み上げ。`on`が偽ならミュート。 */
  speak: { on: boolean; ready: boolean; onToggle: () => void };
  draft: string;
  onDraft: (text: string) => void;
  onSubmit: () => void;
  placeholder: string;
  inputDisabled: boolean;
  sendDisabled: boolean;
  /** 処理中。送信ボタンが停止ボタンに変わる。 */
  busy: boolean;
  /** 処理中の強制中断。 */
  onStop: () => void;
  model: { route: Route | null; models: ModelInfo[]; keyStatus: Partial<KeyStatus>; onChange: (route: Route) => void };
  /** スキルの指定。`value`が`null`なら自動(発話に合うものを選ぶ)。スキルが無ければ渡さない。 */
  skill?: { value: string | null; names: string[]; onChange: (name: string | null) => void };
};

/** 下部の入力カード。待機・マイク・ミュートの操作と、メッセージ入力欄(モデルの切り替えと送信/停止つき)。 */
export default function ChatComposer({ standby, mic, speak, draft, onDraft, onSubmit, placeholder, inputDisabled, sendDisabled, busy, onStop, model, skill }: Props) {
  const t = useT();
  const recording = mic.state === "recording";
  return (
    <div className="hud-panel hud-accent shrink-0 px-4 py-3">
      <div className="mx-auto flex max-w-4xl items-center gap-4">
        <Button
          type="button"
          variant="outline"
          size="icon-lg"
          aria-pressed={standby.on}
          aria-label={standby.on ? t("待機中") : t("待機オフ")}
          disabled={!standby.available}
          title={standby.available ? t("呼びかけで起動する常時待機のオン/オフ") : t("設定で起動ワードを登録してください")}
          className={cn("size-11 shrink-0 rounded-full [&_svg]:size-5", standby.on && "border-primary text-primary")}
          onClick={standby.onToggle}
        >
          <Ear />
        </Button>
        <Button
          type="button"
          size="icon-lg"
          variant={recording ? "destructive" : "outline"}
          aria-label={recording ? t("録音を停止して送信") : t("音声入力")}
          title={t("{recording} ({micHint}を押している間)", { recording: recording ? "録音を停止して送信" : "音声入力", micHint: mic.hint })}
          aria-pressed={recording}
          disabled={mic.disabled}
          className={cn("size-16 shrink-0 rounded-full [&_svg]:size-7", recording && "animate-pulse")}
          onClick={mic.onToggle}
        >
          {mic.state === "starting" || mic.state === "transcribing" ? <Loader2 className="animate-spin" /> : recording ? <Square /> : <Mic />}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="icon-lg"
          aria-pressed={!speak.on}
          aria-label={speak.on ? t("ミュート") : t("読み上げ")}
          disabled={!speak.ready}
          title={speak.ready ? t("応答の読み上げのオン/オフ") : t("設定で読み上げの準備をしてください")}
          className="size-11 shrink-0 rounded-full [&_svg]:size-5"
          onClick={speak.onToggle}
        >
          {speak.on ? <Volume2 /> : <VolumeX />}
        </Button>
        <form
          className="min-w-0 flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            if (busy) onStop();
            else onSubmit();
          }}
        >
          <div className="flex flex-col rounded-lg border border-primary/30 bg-background/60 focus-within:border-primary">
            <Textarea
              value={draft}
              rows={1}
              onChange={(e) => onDraft(e.target.value)}
              onKeyDown={(e) => {
                // Enterで送信、Shift+Enterで改行。日本語入力の変換確定のEnterでは送らない
                if (e.key !== "Enter" || e.shiftKey) return;
                e.preventDefault();
                if (!e.nativeEvent.isComposing && e.keyCode !== 229 && !busy) onSubmit();
              }}
              disabled={inputDisabled}
              placeholder={placeholder}
              aria-label={t("メッセージ")}
              className="max-h-48 min-h-10 resize-none border-0 bg-transparent shadow-none focus-visible:ring-0 dark:bg-transparent"
            />
            <div className="flex items-center justify-between gap-2 px-2 pb-2">
              <div className="flex min-w-0 items-center gap-2">
                <div className="min-w-0 max-w-64">
                  <ModelPicker compact disabled={busy} route={model.route} models={model.models} keyStatus={model.keyStatus} onChange={model.onChange} />
                </div>
                {skill && (
                  <select
                    value={skill.value ?? ""}
                    disabled={busy}
                    aria-label={t("スキル")}
                    title={t("この依頼に使うスキル。「自動」では、発話に合うものがあれば候補になります")}
                    onChange={(e) => skill.onChange(e.target.value === "" ? null : e.target.value)}
                    className="h-7 min-w-0 max-w-40 truncate rounded-md border border-primary/30 bg-background/60 px-2 text-xs text-muted-foreground focus:border-primary focus:outline-none"
                  >
                    <option value="">{t("スキル: 自動")}</option>
                    {skill.names.map((n) => (
                      <option key={n} value={n}>
                        {n}
                      </option>
                    ))}
                  </select>
                )}
              </div>
              {busy ? (
                <Button type="submit" size="icon-sm" variant="destructive" aria-label={t("中断")} title={t("処理を中断")} className="shrink-0">
                  <Square />
                </Button>
              ) : (
                <Button type="submit" size="icon-sm" variant="outline" aria-label={t("送信")} title={t("送信")} disabled={sendDisabled} className="shrink-0">
                  <Send />
                </Button>
              )}
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
