"use client";
import { LANGS, setLang, tr, useLang, useT, type Lang } from "@/lib/i18n";
import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { Archive, AudioLines, Bell, Copy, BookOpen, Keyboard, Pencil, Share2, HardDriveDownload, Volume2, Briefcase, ClipboardList, Download, FolderKanban, Languages, Link2, ListOrdered, Loader2, Mic, Plus, ScrollText, Send, Settings2, ShieldCheck, SlidersHorizontal, SpellCheck, Square, Timer, Upload, UserRound, X, type LucideIcon } from "lucide-react";
import { toast } from "sonner";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { refreshWorkspace, switchTo } from "@/lib/workspace";
import ModelSelector from "@/components/ModelSelector";
import HelpDrawer from "@/components/HelpDrawer";
import { ensureOsPermission, osNotificationsEnabled, setOsNotificationsEnabled, syncOsNotifications } from "@/lib/osNotify";
import NotePanel from "@/components/NotePanel";
import { getSharedData, saveSharedData, LEADER_COLOR, LEADER_ID, createLeader, errorMessage, leaderExists, setWorkEnabled, workEnabled, type KeyStatus, setGlobalLanguage, exportGlobalSettings, importGlobalSettings, type VoiceTiming, getBaseInstructions, getProfile, saveProfile, clearProfile, type UserProfile, getWakeConfig, getBatchInterval, setBatchInterval, listModels, loadProofreadRoute, saveProofreadRoute, type ModelInfo, type Route, type WakeConfig, reportError, saveBaseInstructions, type TtsEngine, setWakeConfig, startRecording, cancelRecording, transcribeWakeSample } from "@/lib/api";

/** 見出しの左に出すアイコン(英字の見出しごと)。無いものは歯車。 */
const HEADING_ICONS: Record<string, LucideIcon> = {
  "Base Prompt": ScrollText,
  "Personal Data": UserRound,
  "Knowledge Interval": Timer,
  Language: Languages,
  Proofread: SpellCheck,
  "Wake Words": Mic,
  "Voice Timing": AudioLines,
  Limits: SlidersHorizontal,
  Backup: Archive,
  Work: Briefcase,
  "OS Notifications": Bell,
  "Leader Prompt": ClipboardList,
  "Space Data": FolderKanban,
  "AI Link": Link2,
  "New Space": Plus,
  "Copy Settings": Copy,
  "Edit Space": Pencil,
  "Shared Data": Share2,
  Spaces: FolderKanban,
  Shortcuts: Keyboard,
  "Voice Guide": BookOpen,
  "Voice Output": Volume2,
  "Voice Input": Mic,
  "Model Sources": HardDriveDownload,
};

/** オン/オフなどの状態の小さな表示。オンは発光し、オフは控えめに出す。 */
export function StatusPill({ on, children }: { on: boolean; children: React.ReactNode }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[0.65rem] font-medium tracking-wide",
        on ? "border-primary/50 bg-primary/10 text-primary" : "border-border text-muted-foreground",
      )}
    >
      <span className={cn("size-1.5 rounded-full", on ? "bg-primary shadow-[0_0_6px_var(--primary)]" : "bg-muted-foreground/50")} />
      {children}
    </span>
  );
}

/** セクション見出し。アイコン、名前、「?」(詳しい説明を開く)、右端の状態(`status`)を並べる。 */
export function Heading({ title, help, status, children }: { title: string; help: string; status?: React.ReactNode; children: React.ReactNode }) {
  const Icon = HEADING_ICONS[title] ?? Settings2;
  return (
    <div className="mb-2 flex items-center gap-2">
      <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
        <Icon className="size-3.5" />
      </span>
      <h2 className="hud-label">{title}</h2>
      <HelpDrawer title={help}>{children}</HelpDrawer>
      {status && <span className="ml-auto">{status}</span>}
    </div>
  );
}

/** 2カラム画面(ナレッジ・デバッグ)の、左カラムの見出し。アイコン、ラベル、右端の操作、その下に状態のチップを並べる。 */
export function PaneHeader({ icon: Icon, title, action, chips }: { icon: LucideIcon; title: string; action?: React.ReactNode; chips?: React.ReactNode }) {
  return (
    <div className="border-b px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
          <Icon className="size-3.5" />
        </span>
        <h1 className="hud-label">{title}</h1>
        {action && <span className="ml-auto">{action}</span>}
      </div>
      {chips && <div className="mt-2 flex flex-wrap gap-1.5">{chips}</div>}
    </div>
  );
}

/** 空状態。アイコンと一言を中央に置く。 */
export function EmptyState({ icon: Icon, children }: { icon: LucideIcon; children: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-3 px-6 py-16 text-center text-sm text-muted-foreground">
      <span className="flex size-12 items-center justify-center rounded-full border border-dashed border-primary/40 text-primary/70">
        <Icon className="size-5" />
      </span>
      {children}
    </div>
  );
}

/** ページの見出し。ラベル・タイトル・説明と、いまの状態をまとめた小さなバッジ(`chips`)を並べる。 */
export function PageHeader({ label, title, description, chips }: { label: string; title: string; description: string; chips?: React.ReactNode }) {
  return (
    <div>
      <p className="hud-label mb-1">{label}</p>
      <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
      <p className="mt-2 text-sm text-muted-foreground">{description}</p>
      {chips && <div className="mt-3 flex flex-wrap gap-2">{chips}</div>}
    </div>
  );
}

export type TuningField<T> = { key: keyof T & string; label: string; hint: string; unit: string; min: number; max: number; step: number };

/** 数値の調整項目の入力欄。入力欄を離れたときに自動で保存する。保存時は他のセクションが保存した項目を上書きしないよう、最新を読み直して自分の項目だけ差し替える。 */
export function TuningPanel<T extends Record<string, number>>({
  title,
  help,
  helpBody,
  fields,
  load,
  store,
  defaults,
}: {
  title: string;
  help: string;
  helpBody: React.ReactNode;
  fields: TuningField<T>[];
  load: () => Promise<T>;
  store: (value: T) => Promise<T>;
  defaults: T;
}) {
  const t = useT();
  const [saved, setSaved] = useState<T | null>(null);
  const [draft, setDraft] = useState<Partial<Record<keyof T & string, string>>>({});
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    load()
      .then(setSaved)
      .catch((e) => toast.error(reportError(e)));
  }, [load]);

  const valueOf = (f: TuningField<T>) => draft[f.key] ?? String(saved?.[f.key] ?? defaults[f.key]);
  const dirty = fields.some((f) => draft[f.key] !== undefined && Number(draft[f.key]) !== saved?.[f.key]);

  const save = async (reset: boolean) => {
    setBusy(true);
    try {
      const latest = await load();
      const next = { ...latest };
      for (const f of fields) {
        const n = reset ? defaults[f.key] : Number(valueOf(f));
        if (Number.isFinite(n)) next[f.key] = n as T[keyof T & string];
      }
      const stored = await store(next);
      setSaved(stored);
      setDraft({});
      toast.success(reset ? t("既定値に戻しました。") : t("保存しました。"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title={title} help={help}>
        {helpBody}
      </Heading>
      <div className="divide-y divide-border">
        {fields.map((f) => (
          <div key={f.key} className="flex items-center gap-3 py-3">
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium">{t(f.label)}</div>
              <div className="text-xs text-muted-foreground">{t(f.hint)}</div>
            </div>
            <Input
              type="number"
              inputMode="decimal"
              className="w-24 text-right"
              min={f.min}
              max={f.max}
              step={f.step}
              disabled={!saved}
              value={valueOf(f)}
              onChange={(e) => setDraft((d) => ({ ...d, [f.key]: e.target.value }))}
              onBlur={() => dirty && !busy && save(false)}
              onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
              aria-label={t(f.label)}
            />
            <span className="w-8 text-xs text-muted-foreground">{t(f.unit)}</span>
          </div>
        ))}
      </div>
      <div className="mt-2 flex justify-end gap-2">
        <Button type="button" variant="ghost" size="sm" disabled={busy || !saved} onClick={() => save(true)}>
          {t("既定に戻す")}
        </Button>
      </div>
    </section>
  );
}

export const VOICE_FIELDS: TuningField<VoiceTiming>[] = [
  { key: "voice_idle_secs", label: "無音で入力を終える時間", hint: "声が入らないとき、音声入力を終えるまでの時間。", unit: "秒", min: 1, max: 30, step: 0.5 },
  { key: "utterance_silence_secs", label: "話し終わりの無音", hint: "会話の続きの待ち受けで、この無音が続くと話し終えたとみなす。", unit: "秒", min: 0.5, max: 5, step: 0.1 },
  { key: "release_tail_secs", label: "スペースを離したあとの保持", hint: "キーを離してから録音を止めるまで。語尾が切れるなら長くする。", unit: "秒", min: 0, max: 3, step: 0.5 },
];

const BATCH_INTERVALS = [5, 15, 30, 60, 180, 360, 720, 1440];
const intervalLabel = (m: number) => (m < 60 ? tr("{m}分", { m }) : tr("{m}時間", { m: m / 60 }));

/** ログ整理(ナレッジ化)を自動実行する間隔。 */
export function BatchInterval() {
  const t = useT();
  const [minutes, setMinutes] = useState<number | null>(null);
  useEffect(() => {
    getBatchInterval()
      .then(setMinutes)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const change = async (next: number) => {
    const prev = minutes;
    setMinutes(next);
    try {
      await setBatchInterval(next);
    } catch (e) {
      setMinutes(prev);
      toast.error(reportError(e));
    }
  };

  const options = minutes !== null && !BATCH_INTERVALS.includes(minutes) ? [...BATCH_INTERVALS, minutes].sort((a, b) => a - b) : BATCH_INTERVALS;

  return (
    <section className="hud-panel hud-accent flex items-center justify-between gap-4 px-5 py-4">
      <div>
        <Heading title="Knowledge Interval" help={t("ログ整理の間隔")}>
          <p>{t("会話ログをトピック別のナレッジへ整理する間隔です。")}</p>
          <ul>
            <li>{t("整理が終わった会話のキャプチャは削除されます。")}</li>
            <li>{t("アプリの終了時と、ナレッジ画面の「今すぐ整理」でも整理します。")}</li>
            <li>{t("変更は次の待ち時間から反映されます。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("この間隔でログをナレッジへ整理します。")}</p>
      </div>
      <select
        aria-label={t("ログ整理の間隔")}
        className="h-9 rounded-md border bg-background px-2 text-sm"
        value={minutes ?? ""}
        disabled={minutes === null}
        onChange={(e) => change(Number(e.target.value))}
      >
        {options.map((m) => (
          <option key={m} value={m}>
            {intervalLabel(m)}
          </option>
        ))}
      </select>
    </section>
  );
}

/** 表示言語。全体設定として保存され、全スペースに反映される(この端末にも写しを残す)。 */
export function LanguageSetting() {
  const t = useT();
  const lang = useLang();
  return (
    <section className="hud-panel hud-accent flex items-center justify-between gap-4 px-5 py-4">
      <div>
        <Heading title="Language" help={t("表示言語")}>
          <p>{t("画面とメニューバーの表示言語を切り替えます。")}</p>
          <ul>
            <li>{t("AIの応答や音声認識の言語は変わりません。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("画面とメニューバーの表示言語")}</p>
      </div>
      <Select items={LANGS} value={lang} onValueChange={(v) => {
          if (!v) return;
          setLang(v as Lang);
          setGlobalLanguage(v as Lang).catch((e) => toast.error(reportError(e)));
        }}>
        <SelectTrigger className="w-36" aria-label={t("表示言語")}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {LANGS.map((l) => (
            <SelectItem key={l.value} value={l.value}>{l.label}</SelectItem>
          ))}
        </SelectContent>
      </Select>
    </section>
  );
}

/** アプリが見えていない間に、ほかのスペースで対応が必要になったとき、macOSの通知も出すか。全体設定。 */
export function OsNotifications() {
  const t = useT();
  const [on, setOn] = useState(true);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setOn(osNotificationsEnabled()); // 静的エクスポートのため、保存値はマウント後に読む
    syncOsNotifications().then(() => setOn(osNotificationsEnabled())).catch(() => {});
  }, []);
  const change = async (next: boolean) => {
    if (next && !(await ensureOsPermission())) {
      toast.error(t("通知が許可されていません。macOSのシステム設定の「通知」で、このアプリを許可してください。"));
      return;
    }
    try {
      await setOsNotificationsEnabled(next);
      setOn(next);
    } catch (e) {
      toast.error(reportError(e));
    }
  };
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="OS Notifications" help={t("OSの通知")} status={<StatusPill on={on}>{on ? t("オン") : t("オフ")}</StatusPill>}>
        <p>{t("アプリが背面にあるときや、ウィンドウを閉じているときに、ほかのスペースの承認待ち・完了・失敗を、macOSの通知でも知らせます。")}</p>
      </Heading>
      <label className="flex items-center justify-between gap-4 text-sm">
        <span className="text-xs text-muted-foreground">
          {t("アプリが背面にあるときや、ウィンドウを閉じているときに、ほかのスペースの承認待ち・完了・失敗を、macOSの通知でも知らせます。")}
        </span>
        <Switch checked={on} onCheckedChange={(v) => void change(v)} />
      </label>
    </section>
  );
}

const PROFILE_FIELDS: {
  key: Exclude<keyof UserProfile, "enabled">;
  label: string;
  placeholder: string;
  type?: string;
}[] = [
  { key: "name", label: "名前", placeholder: "山田 太郎" },
  { key: "nickname", label: "呼ばれ方", placeholder: "たろう" },
  { key: "birthday", label: "生年月日", placeholder: "1990-05-20", type: "date" },
  { key: "location", label: "居住地", placeholder: "東京都" },
  { key: "timezone", label: "タイムゾーン", placeholder: "Asia/Tokyo" },
  { key: "language", label: "使用言語", placeholder: "ja" },
];

/** AIへ毎回渡す本人の基本情報。手動入力のみで、会話から自動では記録しない。 */
export function PersonalData() {
  const t = useT();
  const [saved, setSaved] = useState<UserProfile | null>(null);
  const [value, setValue] = useState<UserProfile | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    getProfile()
      .then((p) => {
        setSaved(p);
        setValue(p);
      })
      .catch((e) => toast.error(reportError(e)));
  }, []);

  if (!value || !saved) return null;
  const dirty = JSON.stringify(value) !== JSON.stringify(saved);

  /** 実行する。入力中の欄を上書きしないよう、sync が true のときだけ画面の値も更新する。 */
  const run = async (action: () => Promise<UserProfile>, ok: string, sync = true) => {
    setBusy(true);
    try {
      const p = await action();
      setSaved(p);
      if (sync) setValue(p);
      toast.success(ok);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Personal Data" help={t("パーソナルデータ")}>
        <p>{t("あなたの基本情報をAIへ毎回渡し、呼びかけや回答を合わせてもらいます。")}</p>
        <ul>
          <li>{t("入力した項目だけが、選んでいるAIのプロバイダ(AnthropicまたはAmazon Bedrock)へ送られます。")}</li>
          <li>{t("生年月日は送らず、年齢だけを渡します。")}</li>
          <li>{t("会話から自動では記録しません。次の送信から反映されます。")}</li>
          <li>{t("設定のエクスポートに含まれます。ファイルの扱いに注意してください。")}</li>
        </ul>
      </Heading>
      <div className="mb-3 flex items-center justify-between gap-4">
        <p className="text-xs text-muted-foreground">{t("入力した内容はAIのプロバイダへ送信されます。")}</p>
        <Switch
          checked={value.enabled}
          disabled={busy}
          onCheckedChange={(enabled) => {
            const next = { ...value, enabled };
            setValue(next);
            run(() => saveProfile(next), t("保存しました。"), false);
          }}
          aria-label={t("パーソナルデータをAIへ渡す")}
        />
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        {PROFILE_FIELDS.map((f) => (
          <label key={f.key} className="grid gap-1 text-xs text-muted-foreground">
            {t(f.label)}
            <Input
              type={f.type}
              value={value[f.key] ?? ""}
              placeholder={t(f.placeholder)}
              onChange={(e) => setValue({ ...value, [f.key]: e.target.value || null })}
              onBlur={() => dirty && !busy && run(() => saveProfile(value), t("保存しました。"), false)}
              onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
            />
          </label>
        ))}
      </div>
      <div className="mt-3 flex justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={busy}
          onClick={() => run(async () => (await clearProfile(), getProfile()), t("削除しました。"))}
        >
          {t("すべて削除")}
        </Button>
      </div>
    </section>
  );
}

/** LLMへ毎回先頭で渡すベース指示の編集欄。入力欄を離れると自動で保存し、空にすると既定値に戻る。 */
export function BaseInstructions() {
  const t = useT();
  const [saved, setSaved] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    getBaseInstructions()
      .then((t) => {
        setSaved(t);
        setValue(t);
      })
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const save = async (text: string, ok: string) => {
    setBusy(true);
    try {
      await saveBaseInstructions(text);
      const t = await getBaseInstructions();
      setSaved(t);
      setValue(t);
      toast.success(ok);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Base Prompt" help={t("ベース指示")}>
        <p>{t("すべての質問でAIへ最初に渡す指示です。")}</p>
        <ul>
          <li>{t("応答はそのまま音声になるため、記号を使わない指定などを入れておくと便利です。")}</li>
          <li>{t("次の送信から反映されます。")}</li>
          <li>{t("空にすると既定の指示に戻ります。")}</li>
        </ul>
      </Heading>
      <p className="mb-3 text-xs text-muted-foreground">{t("AIへ毎回渡す指示です。")}</p>
      <Textarea
        rows={8}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => value !== saved && !busy && save(value, t("保存しました。"))}
        aria-label={t("ベース指示")}
      />
      <div className="mt-3 flex justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={busy}
          onClick={async () => {
            await save("", t("既定の指示に戻しました。"));
          }}
        >
          {t("既定に戻す")}
        </Button>
      </div>
    </section>
  );
}

/** 文字起こし後の校正に使うモデルの登録。オフの間は校正せず、そのまま送信する。 */
export function Proofread({ keyStatus }: { keyStatus: Partial<KeyStatus> }) {
  const t = useT();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [route, setRoute] = useState<Route | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    Promise.all([listModels(), loadProofreadRoute()])
      .then(([m, r]) => {
        setModels(m);
        setRoute(r);
      })
      .catch((e) => toast.error(reportError(e)));
  }, [keyStatus]);

  const usable = models.filter((m) => keyStatus[m.provider]);
  const save = async (next: Route | null) => {
    setBusy(true);
    try {
      await saveProofreadRoute(next);
      setRoute(next);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };
  // 校正は短い文を毎回処理するため、有効化時は軽量モデルを初期値にする
  const initial = usable.find((m) => m.tier === "light") ?? usable[0];

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Proofread" help={t("文字起こしの校正")}>
        <p>{t("音声認識の結果を、直近の会話と過去の訂正履歴を手がかりにAIで校正してから送信します。")}</p>
        <ul>
          <li>{t("発話のたびに選んだモデルが呼ばれ、トークン課金の対象になります。")}</li>
          <li>{t("校正は軽量モデルが向いています。")}</li>
          <li>{t("失敗時は校正前の文章をそのまま使います。")}</li>
        </ul>
      </Heading>
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">{t("文字起こしを文脈から校正する")}</div>
          <div className="text-xs text-muted-foreground">
            {t("認識結果をAIで校正してから送信します(発話ごとに課金)。")}
          </div>
        </div>
        <Switch
          aria-label={t("文字起こしの校正")}
          checked={!!route}
          disabled={busy || (!route && !initial)}
          onCheckedChange={(on) => save(on && initial ? { provider: initial.provider, model: initial.id } : null)}
        />
      </div>
      {route && (
        <div className="mt-3">
          <ModelSelector route={route} models={models} keyStatus={keyStatus} onChange={save} />
        </div>
      )}
      {!route && !initial && (
        <p className="mt-2 text-xs text-muted-foreground">{t("先にAIのAPIキーを接続してください。")}</p>
      )}
    </section>
  );
}

/** 候補の同一判定用。ひらがな・カタカナ・全角半角・句読点の違いを揃える(待機中の検出と同じ基準)。 */
const foldWake = (text: string) =>
  text
    .normalize("NFKC")
    .replace(/[\u30A1-\u30F6]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0x60))
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]/gu, "");

/** 認識結果から候補の表記を作る(前後の句読点・空白を除く)。 */
const wakeCandidate = (text: string) => text.trim().replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");

type WakeSample = { word: string; count: number };

export function WakeWords({ ttsReady, engine, cloudStt }: { ttsReady: boolean; engine: TtsEngine; cloudStt: string | null }) {
  const t = useT();
  const [config, setConfig] = useState<WakeConfig | null>(null);
  const [word, setWord] = useState("");
  const [busy, setBusy] = useState(false);
  const [rec, setRec] = useState<"idle" | "recording" | "transcribing">("idle");
  const [samples, setSamples] = useState<WakeSample[]>([]);
  const [takes, setTakes] = useState(0);
  useEffect(() => {
    getWakeConfig()
      .then(setConfig)
      .catch((e) => toast.error(reportError(e)));
  }, []);
  const recording = rec === "recording";
  const recordingRef = useRef(false);
  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);
  // 録音したまま画面を離れたら破棄する
  useEffect(() => () => void (recordingRef.current && cancelRecording()), []);

  const toggleRecording = async () => {
    if (rec === "recording") {
      setRec("transcribing");
      try {
        const text = wakeCandidate(await transcribeWakeSample());
        const key = foldWake(text);
        setTakes((n) => n + 1);
        if (key) {
          setSamples((prev) => {
            const hit = prev.find((x) => foldWake(x.word) === key);
            return hit ? prev.map((x) => (x === hit ? { ...x, count: x.count + 1 } : x)) : [...prev, { word: text, count: 1 }];
          });
        }
      } catch (e) {
        const code = (e as { code?: string })?.code;
        toast.error(code === "too_short" || code === "no_audio" ? t("声が聞き取れませんでした。もう一度録音してください。") : reportError(e));
      } finally {
        setRec("idle");
      }
      return;
    }
    try {
      await startRecording();
      setRec("recording");
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  const save = async (next: WakeConfig) => {
    setBusy(true);
    try {
      const saved = await setWakeConfig(next);
      setConfig(saved);
      return saved;
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    const w = word.trim();
    if (!w || !config) return;
    const saved = await save({ ...config, words: [...config.words, w] });
    if (!saved) return;
    if (saved.words.length > config.words.length) setWord("");
    else toast.error(t("追加できませんでした。2〜20文字で、すでにある語(ひらがな・カタカナの違いを除く)とは別の語にしてください。登録は10個までです。"));
  };

  const addSample = async (w: string) => {
    if (!config) return;
    const saved = await save({ ...config, words: [...config.words, w] });
    if (!saved) return;
    if (saved.words.length > config.words.length) setSamples((prev) => prev.filter((x) => x.word !== w));
    else toast.error(t("追加できませんでした。2〜20文字で、すでにある語(ひらがな・カタカナの違いを除く)とは別の語にしてください。登録は10個までです。"));
  };

  /** 候補をまとめて追加する。検証を通って保存された語だけ候補から外し、残りは理由を伝える。 */
  const addAllSamples = async () => {
    if (!config) return;
    const words = [...samples].sort((a, b) => b.count - a.count).map((x) => x.word);
    const saved = await save({ ...config, words: [...config.words, ...words] });
    if (!saved) return;
    const kept = new Set(saved.words.map(foldWake));
    setSamples((prev) => prev.filter((x) => !kept.has(foldWake(x.word))));
    const added = saved.words.length - config.words.length;
    if (added < words.length) toast.error(t("{added}語を追加しました。残りは2〜20文字の範囲外、すでにある語との重複、または登録数の上限(10個)のため追加できませんでした。", { added }));
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <Heading title="Wake Words" help={t("常時音声待機")} status={<StatusPill on={config?.enabled ?? false}>{config?.enabled ? t("待機中") : t("オフ")}</StatusPill>}>
            <p>{t("対話画面を開いている間マイクを開いたままにし、起動ワードを含む発話があると「用件は、なんでしょうか」と応えて依頼を待ちます。「タチコマ、今日の予定は」のように続けて話すと、そのまま依頼として送ります。")}</p>
            <ul>
              <li>{t("判定はAIによる校正より前に行います。起動ワードを含まない発話はAIへ送られず、課金されません。")}</li>
              <li>
                {t("待機中は発話のたびに音声認識が動きます。")}
                {cloudStt ? t("{cloudStt}を使っているため、その分が課金対象になります。ローカルのWhisperなら無料です。", { cloudStt }) : t("ローカルのWhisperなら無料です。")}
              </li>
              <li>
                {t("応答の音声は、Voice Linkの「定型音声を作成」で先に作成して保存したものを再生します(待機中にTTSへは送りません)。")}
                {ttsReady ? "" : engine === "voicevox" ? t("VOICEVOXが未取得のため、音声は作成できません。") : t("Google Cloudのキーが未登録のため、音声は作成できません。")}
              </li>
              <li>{t("応答や読み上げの再生中は、その音を拾わないよう待機を止めます。")}</li>
              <li>{t("ひらがな・カタカナ・全角半角・句読点の違いは区別しません。1語は2〜20文字、登録は10個までです。")}</li>
            </ul>
          </Heading>
          <p className="text-xs text-muted-foreground">{t("呼びかけで起動し、「用件は、なんでしょうか」と応えます。")}</p>
        </div>
        <Switch
          aria-label={t("常時音声待機")}
          checked={config?.enabled ?? false}
          disabled={!config || busy || !config.words.length}
          onCheckedChange={(on) => config && save({ ...config, enabled: on })}
        />
      </div>
      <form
        className="mt-3 flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <Input
          placeholder={t("起動ワード(例: タチコマ)")}
          aria-label={t("起動ワード")}
          maxLength={20}
          value={word}
          onChange={(e) => setWord(e.target.value)}
        />
        <Button type="submit" variant="outline" disabled={!config || busy || !word.trim()}>
          <Plus /> {t("追加")}
        </Button>
      </form>
      <div className="mt-3 rounded-sm border border-dashed px-3 py-2.5">
        <div className="flex items-center gap-3">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!config || busy || rec === "transcribing"}
            onClick={() => void toggleRecording()}
          >
            {rec === "transcribing" ? <Loader2 className="animate-spin" /> : recording ? <Square /> : <Mic />}
            {recording ? t("録音を止めて認識") : rec === "transcribing" ? t("認識中…") : t("声で録音")}
          </Button>
          <p className="min-w-0 flex-1 text-xs text-muted-foreground">
            {takes
              ? t("{n}回録音しました。認識が揃った候補ほど、待機中も同じ形で聞き取られやすい語です。", { n: takes })
              : t("起動ワードを声に出して、同じ言葉で数回録音してください。音声認識の結果が候補になります。")}
          </p>
          {samples.length > 1 && (
            <Button type="button" variant="outline" size="sm" disabled={busy || recording || rec === "transcribing"} onClick={() => void addAllSamples()}>
              {t("すべて追加")}
            </Button>
          )}
          {!!takes && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={recording || rec === "transcribing"}
              onClick={() => {
                setSamples([]);
                setTakes(0);
              }}
            >
              {t("クリア")}
            </Button>
          )}
        </div>
        {!!samples.length && (
          <ul className="mt-2 flex flex-wrap gap-2">
            {[...samples]
              .sort((a, b) => b.count - a.count)
              .map((x) => (
                <li key={x.word} className="flex items-center gap-1 rounded-sm border bg-muted/40 py-0.5 pr-1 pl-2.5 text-sm">
                  {x.word}
                  <span className="text-xs text-muted-foreground">×{x.count}</span>
                  <Button type="button" variant="ghost" size="icon-xs" aria-label={t("{w}を起動ワードに追加", { w: x.word })} disabled={busy} onClick={() => void addSample(x.word)}>
                    <Plus />
                  </Button>
                  <Button type="button" variant="ghost" size="icon-xs" aria-label={t("{w}を候補から外す", { w: x.word })} onClick={() => setSamples((prev) => prev.filter((y) => y !== x))}>
                    <X />
                  </Button>
                </li>
              ))}
          </ul>
        )}
      </div>
      {!!config?.words.length && (
        <ul className="mt-3 flex flex-wrap gap-2">
          {config.words.map((w) => (
            <li key={w} className="flex items-center gap-1 rounded-sm border bg-muted/40 py-0.5 pr-1 pl-2.5 text-sm">
              {w}
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={t("{w}を削除", { w })}
                disabled={busy}
                onClick={() => save({ ...config, words: config.words.filter((x) => x !== w) })}
              >
                <X />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}


/** 全体設定だけのバックアップ。スペースごとの設定のバックアップ(設定画面の Backup)とは別に書き出す。 */
export function GlobalBackup() {
  const t = useT();
  const [busy, setBusy] = useState(false);

  const exportAll = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const dest = await saveDialog({
      title: t("全体設定のエクスポート"),
      defaultPath: `myassistant-global-settings-${stamp}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!dest) return;
    setBusy(true);
    try {
      const r = await exportGlobalSettings(dest);
      toast.success(t("{applied}項目を書き出しました。", { applied: r.applied }));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  const importAll = async () => {
    const src = await openDialog({
      title: t("全体設定のインポート"),
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof src !== "string") return;
    setBusy(true);
    try {
      const r = await importGlobalSettings(src);
      toast.success(t("{applied}項目を読み込みました。", { applied: r.applied }));
      // 各項目が保持している画面上の値を、読み込んだ内容に合わせ直す
      setTimeout(() => window.location.reload(), 600);
    } catch (e) {
      toast.error(reportError(e));
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Backup" help={t("全体設定の移行")}>
        <p>{t("全体設定をJSONファイルで書き出し・読み込みできます。")}</p>
        <ul>
          <li>{t("スペースごとの設定、APIキー、ナレッジは含まれません。それらは各スペースの設定のバックアップを使ってください。")}</li>
          <li>{t("読み込むと、ファイルにある項目が現在の全体設定を上書きし、すべてのスペースに反映されます。")}</li>
        </ul>
      </Heading>
      <div className="mt-3 flex gap-2">
        <Button variant="outline" size="sm" disabled={busy} onClick={exportAll}>
          <Download /> {t("エクスポート")}
        </Button>
        <Button variant="outline" size="sm" disabled={busy} onClick={importAll}>
          {busy ? <Loader2 className="animate-spin" /> : <Upload />} {t("インポート")}
        </Button>
      </div>
    </section>
  );
}

/**
 * ワークの有効化・無効化。
 * - 未作成: ワークの説明(何か・できること・ルール)を示し、承諾したらリーダーを作成して使えるようにする(承諾までは何も作らない)。
 * - 有効: 「ワークを無効にする」で、画面から選べなくする。リーダー・スペースのデータ(会話・ナレッジ・設定・APIキー・台帳)は残る。
 * - 無効: 「ワークを有効にする」で、承諾し直さずに元へ戻す。
 */
/** 共通データ。全スペースで共通して、AIへ毎回渡す自由記述の指示(パーソナルには渡さない)。 */
export function SharedData() {
  const t = useT();
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Shared Data" help={t("共通データ")}>
        <p>{t("すべてのスペースで共通して、AIへ毎回渡す指示です(パーソナルには渡しません)。口調や守ってほしいルールなどを書きます。スペースごとの内容は、各スペースの設定の「スペースデータ」に書きます。次の送信から反映されます。")}</p>
      </Heading>
      <NotePanel load={getSharedData} save={saveSharedData} label={t("共通データ")} placeholder={t("例: 回答は敬語で、結論から先に述べてください。")} />
    </section>
  );
}

export function WorkEnable() {
  const t = useT();
  const router = useRouter();
  const [exists, setExists] = useState<boolean | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [agreed, setAgreed] = useState(false);
  const [confirmOff, setConfirmOff] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    Promise.all([leaderExists(), workEnabled()])
      .then(([e, on]) => {
        setExists(e);
        setEnabled(on);
      })
      .catch((e) => toast.error(errorMessage(e)));
  }, []);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const enable = () =>
    run(async () => {
      await createLeader();
      await refreshWorkspace();
      await switchTo(LEADER_ID);
      setExists(true);
      setEnabled(true);
      router.push("/setup");
      toast.success(t("ワークを有効にしました。まず、リーダーのAIの接続(APIキー)を登録してください。"));
    });

  const turnOff = () =>
    run(async () => {
      await setWorkEnabled(false);
      await refreshWorkspace();
      setEnabled(false);
      setConfirmOff(false);
      toast.success(t("ワークを無効にしました。データは残っています。"));
    });

  const turnOn = () =>
    run(async () => {
      await setWorkEnabled(true);
      await refreshWorkspace();
      setEnabled(true);
      toast.success(t("ワークを有効にしました。"));
    });

  return (
    <section id="work" className="hud-panel hud-accent scroll-mt-6 px-5 py-4">
      <Heading title="Work" help={t("ワーク")} status={exists === null ? undefined : <StatusPill on={exists && enabled}>{!exists ? t("未作成") : enabled ? t("有効") : t("無効")}</StatusPill>}>
        <p>{t("ワークは、承諾するとリーダーを作成して使えるようになります。承諾するまでは、何も作られません。")}</p>
        <ul>
          <li>{t("無効にしても、リーダーとスペースのデータは消えません。画面から選べなくなり、リーダーの配布と、スペースのバックグラウンド処理(ナレッジ整理)が止まります。")}</li>
        </ul>
      </Heading>
      {exists === null ? null : exists && enabled ? (
        <div className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">{t("ワークは有効です。リーダーとスペースは、左端のレールの「WORK」の枠から開きます。")}</p>
          {confirmOff ? (
            <div className="flex flex-col gap-2 rounded-lg border p-3 text-sm">
              <p>{t("ワークを無効にしますか?リーダーとスペースのデータ(会話・ナレッジ・設定・APIキー・台帳)は残り、画面から選べなくなります。開いていれば、パーソナルへ戻ります。実行中の依頼があるときは、完了してから無効にしてください。")}</p>
              <div className="flex gap-2">
                <Button variant="destructive" disabled={busy} onClick={() => void turnOff()}>
                  {t("無効にする")}
                </Button>
                <Button variant="outline" disabled={busy} onClick={() => setConfirmOff(false)}>
                  {t("キャンセル")}
                </Button>
              </div>
            </div>
          ) : (
            <div>
              <Button variant="outline" onClick={() => setConfirmOff(true)}>
                {t("ワークを無効にする")}
              </Button>
            </div>
          )}
        </div>
      ) : exists ? (
        <div className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">{t("ワークは無効です。リーダーとスペースのデータは残っています。有効にすると、元のとおり使えます。")}</p>
          <div>
            <Button disabled={busy} onClick={() => void turnOn()}>
              {t("ワークを有効にする")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-6">
          {/* 見出し: ワークが何かを一文で */}
          <div className="pt-1">
            <h3 className="text-xl font-semibold tracking-tight">{t("仕事ごとに、専属のエージェントを持つ")}</h3>
            <p className="mt-2 text-sm text-muted-foreground">
              {t("リーダーが指示を配り、進捗をひとつにまとめます。スペースは案件ごとに独立して動きます。パーソナルは、ワークなしでも使えます。")}
            </p>
          </div>

          {/* 図解: リーダーと、その下のスペース */}
          <div className="flex flex-col items-center rounded-xl border bg-muted/30 px-4 py-5" aria-hidden="true">
            <div
              className="flex items-center gap-2 rounded-lg border px-3 py-2 text-sm font-semibold"
              style={{ borderColor: LEADER_COLOR, color: LEADER_COLOR, backgroundColor: `${LEADER_COLOR}14` }}
            >
              <ClipboardList className="size-4" />
              Leader
              <span className="hidden text-xs font-normal text-muted-foreground sm:inline">{t("指示を配る・進捗を集める")}</span>
            </div>
            <div className="h-4 w-px bg-border" />
            <div className="h-px w-2/3 bg-border" />
            <div className="grid w-full grid-cols-3 gap-2 pt-0">
              {[
                { name: "経理", color: "#19bde6" },
                { name: "営業", color: "#6be619" },
                { name: "開発", color: "#bd19e6" },
              ].map((x) => (
                <div key={x.name} className="flex flex-col items-center">
                  <div className="h-3 w-px bg-border" />
                  <div className="flex w-full items-center justify-center gap-2 rounded-lg border bg-background/60 px-2 py-2 text-sm">
                    <span className="size-3 shrink-0 rounded-full" style={{ backgroundColor: x.color }} />
                    <span className="truncate">{t(x.name)}</span>
                  </div>
                </div>
              ))}
            </div>
            <p className="mt-3 text-xs text-muted-foreground">{t("スペースは、仕事の種類や案件ごとに作る、独立したエージェントです(例)")}</p>
          </div>

          {/* 3枚のカード */}
          <div className="grid gap-3 sm:grid-cols-3">
            {[
              {
                icon: Send,
                title: "できること",
                items: [
                  "リーダーと話して、配る指示の下書きと宛先の案を作れます。",
                  "配布ごとの進捗(実行中・承認待ち・完了・失敗)を、台帳で一か所に確認できます。",
                  "スペースごとに、会話・ナレッジ・設定・APIキーを分けて使えます。",
                ],
              },
              {
                icon: ShieldCheck,
                title: "守られること",
                items: [
                  "リーダーは専用のAPIキーを持ち、スペースの会話・ナレッジ・結果の本文を読みません。",
                  "配布は、内容と宛先を確かめて「配布」を押したときだけ。AIが勝手に配ることはありません。",
                  "会話・ナレッジ・APIキーは、スペース同士でも、パーソナルとも共有されません。",
                ],
              },
              {
                icon: ListOrdered,
                title: "始める前に",
                items: [
                  "最初にリーダーを作成します(承諾すると作成されます)。作らないと、スペースは作れません。",
                  "リーダーのAPIキーを登録します。",
                  "スペースを作ります。最大30件で、名称と色は重複できません。「パーソナル」「ワーク」「リーダー」は予約名です。",
                ],
                numbered: true,
              },
            ].map((c) => (
              <div key={c.title} className="flex flex-col gap-2 rounded-xl border bg-background/40 p-4">
                <div className="flex items-center gap-2">
                  <c.icon className="size-4 text-primary" />
                  <h4 className="text-sm font-semibold">{t(c.title)}</h4>
                </div>
                {c.numbered ? (
                  <ol className="list-decimal space-y-1.5 pl-4 text-xs leading-relaxed text-muted-foreground">
                    {c.items.map((x) => (
                      <li key={x}>{t(x)}</li>
                    ))}
                  </ol>
                ) : (
                  <ul className="list-disc space-y-1.5 pl-4 text-xs leading-relaxed text-muted-foreground">
                    {c.items.map((x) => (
                      <li key={x}>{t(x)}</li>
                    ))}
                  </ul>
                )}
              </div>
            ))}
          </div>

          {/* 承諾バー */}
          <div className="sticky bottom-0 -mx-5 -mb-4 flex flex-wrap items-center justify-between gap-3 border-t bg-background/90 px-5 py-3 backdrop-blur">
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={agreed} onChange={(e) => setAgreed(e.target.checked)} />
              {t("内容を理解しました")}
            </label>
            <Button disabled={!agreed || busy} className={cn(agreed && "neon-glow")} onClick={() => void enable()}>
              {t("承諾してワークを有効にする")}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
