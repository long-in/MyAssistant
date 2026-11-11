"use client";

import { useEffect, useRef, useState } from "react";
import { useT } from "@/lib/i18n";
import { ArrowUp, Ear, Loader2, MessageSquarePlus, Mic, Play, Square, Volume2, VolumeX, X, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import ModelPicker from "./ModelPicker";
import { VoiceOrb } from "./Waveforms";
import { PHASE_HUE, PHASE_LABELS, type Phase } from "./OrbReadout";
import { cn } from "@/lib/utils";
import type { ModelInfo, ModelKeyStatus, Route, Skill } from "@/lib/api";

export type MicState = "idle" | "starting" | "recording" | "transcribing" | "standby";

const APP_VERSION = process.env.NEXT_PUBLIC_APP_VERSION;

type Props = {
  /** 処理段階(待機中・音声入力中・思考中など)。小さな円形波形と状態の帯に出す。 */
  phase: Phase;
  /** 呼びかけで起動する常時待機。 */
  standby: { on: boolean; available: boolean; onToggle: () => void };
  /** 音声入力。`hint`は押している間だけ録音するキーの名前。 */
  mic: { state: MicState; disabled: boolean; hint: string; onToggle: () => void };
  /** 返答の読み上げ。`on`が偽ならミュート。 */
  speak: { on: boolean; ready: boolean; onToggle: () => void };
  /** 新しい会話を始める。 */
  onNewConversation: () => void;
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
  /** モデルの選択。サンドボックスでは、settings.json で許可したモデル(名前の文字列)から選ぶ。 */
  model:
    | { kind: "app"; route: Route | null; models: ModelInfo[]; keyStatus: ModelKeyStatus; onChange: (route: Route) => void }
    | { kind: "sandbox"; models: string[]; selected: string | null; onChange: (model: string) => void };
  /**
   * 入力欄でのスキルの指定。「/」で候補を出し、選ぶと選定を省いてその手順で送る。
   * 渡さなければ(コンテナ内の Claude Code で会話するエージェント)、指定はできない。
   */
  skill?: { list: Skill[]; selected: string | null; onSelect: (name: string | null) => void };
  /**
   * 入力欄の下に、状態の帯(小さな円形波形・処理段階・往復数・コンテナ・時刻)を出すか。
   * 右のパネル(大きな円形波形と情報カード)を畳んでいるあいだだけ出し、同じ情報を二重に見せない。
   */
  showStatus: boolean;
  /** この会話の往復数。 */
  turns: number;
  /** サンドボックスのコンテナの状態と操作(コンテナで会話するときだけ)。 */
  container?: { running: boolean; busy: boolean; disabled: boolean; onToggle: () => void };
};

/** 東京の時刻(時:分)。分が変わるころに更新する。サーバー描画との差を避けるため、表示は読み込み後。 */
function useClock() {
  const [now, setNow] = useState<Date | null>(null);
  useEffect(() => {
    const tick = () => setNow(new Date());
    tick();
    const id = setInterval(tick, 15_000);
    return () => clearInterval(id);
  }, []);
  return now ? now.toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" }) : "--:--";
}

/**
 * 下部の入力カード。コーディング支援のチャットのように、1つの枠の中に入力欄と操作(モデル・音声・送信)をまとめる。
 * - 入力欄で「/」を打つとスキルの候補が開き、↑↓で選んでEnter(またはTab)で指定する。指定したスキルは札で示し、空の入力欄でBackspaceを押すと外れる。
 * - 右のパネルを畳んでいるあいだは、枠の下の帯に、処理段階・往復数・コンテナの状態・時刻を出す。
 */
export default function ChatComposer({
  phase,
  standby,
  mic,
  onNewConversation,
  speak,
  draft,
  onDraft,
  onSubmit,
  placeholder,
  inputDisabled,
  sendDisabled,
  busy,
  onStop,
  model,
  skill,
  showStatus,
  turns,
  container,
}: Props) {
  const t = useT();
  const clock = useClock();
  const recording = mic.state === "recording";
  const input = useRef<HTMLTextAreaElement>(null);

  // 「/」で始まり、まだ空白を含まないあいだは、スキルの候補を出す
  const query = skill && !skill.selected && /^\/[^\s]*$/.test(draft) ? draft.slice(1).toLowerCase() : null;
  const matches = query === null || !skill ? [] : skill.list.filter((s) => s.name.includes(query) || s.description.toLowerCase().includes(query)).slice(0, 8);
  const [cursor, setCursor] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const menuOpen = query !== null && !dismissed;
  const index = Math.min(cursor, Math.max(0, matches.length - 1));
  const pick = (name: string) => {
    skill?.onSelect(name);
    onDraft("");
    setCursor(0);
    input.current?.focus();
  };
  const hue = PHASE_HUE[phase];
  const label = PHASE_LABELS[phase];

  return (
    <div className="shrink-0">
      <div className="mx-auto w-full max-w-3xl">
        <form
          className="relative"
          onSubmit={(e) => {
            e.preventDefault();
            if (busy) onStop();
            else onSubmit();
          }}
        >
          {menuOpen && (
            <div role="listbox" aria-label={t("スキル")} className="absolute inset-x-0 bottom-full z-30 mb-2 max-h-64 overflow-y-auto rounded-md border bg-popover p-1 text-popover-foreground shadow-lg">
              {matches.length === 0 && (
                <p className="px-2 py-2 text-xs text-muted-foreground">{skill && skill.list.length === 0 ? t("スキルがありません。Skills 画面で登録できます。") : t("当てはまるスキルがありません。")}</p>
              )}
              {matches.map((s, i) => (
                <button
                  key={s.name}
                  type="button"
                  role="option"
                  aria-selected={i === index}
                  onMouseEnter={() => setCursor(i)}
                  onClick={() => pick(s.name)}
                  className={cn("flex w-full items-baseline gap-2 rounded-sm px-2 py-1.5 text-left text-xs", i === index && "bg-accent text-accent-foreground")}
                >
                  <span className="shrink-0 font-mono font-medium">/{s.name}</span>
                  <span className="min-w-0 flex-1 truncate text-muted-foreground">{s.description}</span>
                </button>
              ))}
            </div>
          )}
          <div className="flex flex-col rounded-xl border border-border bg-card/70 shadow-sm transition-colors focus-within:border-primary/70">
            {skill?.selected && (
              <div className="px-3 pt-2.5">
                <span className="inline-flex max-w-full items-center gap-1 rounded-md border border-primary/40 bg-primary/10 py-0.5 pr-1 pl-2 font-mono text-xs text-primary">
                  <Zap className="size-3 shrink-0" />
                  <span className="truncate">/{skill.selected}</span>
                  <button type="button" aria-label={t("スキルの指定を外す")} title={t("スキルの指定を外す")} onClick={() => skill.onSelect(null)} className="rounded p-0.5 hover:bg-primary/20">
                    <X className="size-3" />
                  </button>
                </span>
              </div>
            )}
            <Textarea
              ref={input}
              value={draft}
              rows={1}
              onChange={(e) => {
                setDismissed(false);
                setCursor(0);
                onDraft(e.target.value);
              }}
              onKeyDown={(e) => {
                const composing = e.nativeEvent.isComposing || e.keyCode === 229;
                if (menuOpen && !composing) {
                  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                    e.preventDefault();
                    if (matches.length > 0) setCursor((index + (e.key === "ArrowDown" ? 1 : matches.length - 1)) % matches.length);
                    return;
                  }
                  if ((e.key === "Enter" || e.key === "Tab") && matches[index]) {
                    e.preventDefault();
                    pick(matches[index].name);
                    return;
                  }
                  if (e.key === "Escape") {
                    // 候補を閉じるだけにする(画面全体の強制中断へ伝えない)
                    e.preventDefault();
                    e.stopPropagation();
                    setDismissed(true);
                    return;
                  }
                }
                if (e.key === "Backspace" && draft === "" && skill?.selected) {
                  skill.onSelect(null);
                  return;
                }
                // Enterで送信、Shift+Enterで改行。日本語入力の変換確定のEnterでは送らない
                if (e.key !== "Enter" || e.shiftKey) return;
                e.preventDefault();
                if (!composing && !busy) onSubmit();
              }}
              disabled={inputDisabled}
              placeholder={skill ? t("{status}(「/」でスキルを指定)", { status: placeholder }) : placeholder}
              aria-label={t("メッセージ")}
              className="max-h-48 min-h-11 resize-none border-0 bg-transparent px-3 pt-3 shadow-none focus-visible:ring-0 dark:bg-transparent"
            />
            <div className="flex items-center justify-between gap-2 px-2 pb-2">
              <div className="flex min-w-0 items-center gap-1">
                <Button
                  type="button"
                  size="icon-sm"
                  variant={recording ? "destructive" : "ghost"}
                  aria-label={recording ? t("録音を停止して送信") : t("音声入力")}
                  title={t("{recording} ({micHint}を押している間)", { recording: recording ? "録音を停止して送信" : "音声入力", micHint: mic.hint })}
                  aria-pressed={recording}
                  disabled={mic.disabled}
                  className={cn("shrink-0", recording && "animate-pulse")}
                  onClick={mic.onToggle}
                >
                  {mic.state === "starting" || mic.state === "transcribing" ? <Loader2 className="animate-spin" /> : recording ? <Square /> : <Mic />}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-pressed={standby.on}
                  aria-label={standby.on ? t("待機中") : t("待機オフ")}
                  disabled={!standby.available}
                  title={standby.available ? t("呼びかけで起動する常時待機のオン/オフ") : t("設定で起動ワードを登録してください")}
                  className={cn("shrink-0", standby.on && "bg-primary/10 text-primary")}
                  onClick={standby.onToggle}
                >
                  <Ear />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-pressed={!speak.on}
                  aria-label={speak.on ? t("ミュート") : t("読み上げ")}
                  disabled={!speak.ready}
                  title={speak.ready ? t("応答の読み上げのオン/オフ") : t("設定で読み上げの準備をしてください")}
                  className="shrink-0"
                  onClick={speak.onToggle}
                >
                  {speak.on ? <Volume2 /> : <VolumeX />}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t("新しい会話")}
                  title={t("これまでの会話の流れを引き継がず、新しい会話を始める")}
                  className="shrink-0"
                  onClick={onNewConversation}
                >
                  <MessageSquarePlus />
                </Button>
                <div className="min-w-0 max-w-64">
                  {model.kind === "app" ? (
                    <ModelPicker compact disabled={busy} route={model.route} models={model.models} keyStatus={model.keyStatus} onChange={model.onChange} />
                  ) : model.models.length > 0 && model.selected ? (
                    <ModelPicker
                      compact
                      flat
                      disabled={busy}
                      route={{ provider: "anthropic", model: model.selected }}
                      models={model.models.map((id) => ({ provider: "anthropic", id, label: id, tier: "heavy" }))}
                      keyStatus={{ anthropic: true }}
                      onChange={(r) => model.onChange(r.model)}
                    />
                  ) : (
                    <span className="px-2 text-xs text-muted-foreground" title={t("Claude Code の settings.json に availableModels を書くと、ここで選べます")}>
                      {t("Sandbox 設定のモデル")}
                    </span>
                  )}
                </div>
              </div>
              {busy ? (
                <Button type="submit" size="icon-sm" variant="destructive" aria-label={t("中断")} title={t("処理を中断")} className="shrink-0 rounded-full">
                  <Square />
                </Button>
              ) : (
                <Button type="submit" size="icon-sm" aria-label={t("送信")} title={t("送信")} disabled={sendDisabled} className="shrink-0 rounded-full">
                  <ArrowUp />
                </Button>
              )}
            </div>
          </div>
        </form>

        {/* 状態の帯: 処理段階(小さな円形波形つき)・往復数・コンテナ・時刻。右のパネルを畳んでいるあいだだけ出す */}
        <div className={cn("mt-1.5 items-center gap-3 px-1 text-xs text-muted-foreground", showStatus ? "flex" : "hidden")}>
          <span className="flex min-w-0 items-center gap-1.5" aria-live="polite">
            {showStatus && <VoiceOrb phase={phase} micActive={recording} thinking={busy} className="size-7 shrink-0" />}
            <span className={cn("font-mono tracking-wider", phase !== "standby" && "animate-hud-blink-slow")} style={{ color: `hsl(${hue} 80% 55%)` }}>
              {label.en}
            </span>
            <span className="truncate">{t(label.ja)}</span>
          </span>
          <span className="ml-auto flex shrink-0 items-center gap-3">
            {container && (
              <button
                type="button"
                disabled={container.busy || container.disabled}
                title={
                  container.disabled && !container.running
                    ? t("イメージのビルドが必要です。Sandbox 設定を確認してください")
                    : container.running
                      ? t("コンテナを停止")
                      : t("コンテナを起動")
                }
                onClick={container.onToggle}
                className="flex items-center gap-1.5 rounded px-1.5 py-0.5 transition-colors hover:bg-accent hover:text-foreground disabled:pointer-events-none disabled:opacity-60"
              >
                <span
                  aria-hidden
                  className={cn("size-2 rounded-full", container.busy ? "animate-pulse bg-amber-500" : container.running ? "bg-emerald-500 shadow-[0_0_6px] shadow-emerald-500/70" : "bg-muted-foreground/50")}
                />
                {t("コンテナ")}: {container.busy ? t("処理中…") : container.running ? t("稼働中") : t("停止中")}
                {container.busy ? <Loader2 className="size-3 animate-spin" /> : container.running ? <Square className="size-3" /> : <Play className="size-3" />}
              </button>
            )}
            <span className="tabular-nums">{t("{n}往復", { n: turns })}</span>
            <span className="tabular-nums" title={t("東京")}>
              {clock}
            </span>
            {APP_VERSION && <span className="font-mono opacity-60">v{APP_VERSION}</span>}
          </span>
        </div>
      </div>
    </div>
  );
}
