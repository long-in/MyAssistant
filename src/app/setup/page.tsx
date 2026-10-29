"use client";

import { LANGS, setLang, tr, useLang, useT, type Lang } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { ArrowRight, Check, ChevronDown, Download, KeyRound, Loader2, Pencil, Plus, Trash2, Upload, X } from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import ModelSelector from "@/components/ModelSelector";
import HelpDrawer from "@/components/HelpDrawer";
import { cn } from "@/lib/utils";
import {
  PROVIDERS,
  PROVIDER_LABELS,
  type KeyStatus,
  type Provider,
  deleteApiKey,
  aquaKeyRegistered,
  deleteAquaKey,
  deleteTtsKey,
  exportSettings,
  importSettings,
  getBaseInstructions,
  getProfile,
  saveProfile,
  clearProfile,
  type UserProfile,
  DEFAULT_TUNING,
  getKeyStatus,
  getTuning,
  setTuning,
  type Tuning,
  DEBUG_MODE_EVENT,
  type VoicePhrase,
  generateVoicePhrases,
  getCommandPolicy,
  getDefaultCommandPolicy,
  getDebugMode,
  setCommandPolicy,
  type CommandPolicy,
  getShellAccess,
  getTtsDictionary,
  getWakeConfig,
  setTtsDictionary,
  type DictionaryEntry,
  type TtsDictionary,
  getWebAccess,
  getSttEngine,
  getTtsEngine,
  getVoicevoxStyle,
  installVoicevox,
  getBatchInterval,
  setBatchInterval,
  getSttLanguage,
  downloadSttModel,
  sttModelUpgradable,
  setSttLanguage,
  type SttLanguage,
  listModels,
  loadProofreadRoute,
  saveProofreadRoute,
  type ModelInfo,
  type Route,
  type WakeConfig,
  reportError,
  saveApiKey,
  saveBaseInstructions,
  saveAquaKey,
  saveTtsKey,
  setSttEngine,
  setTtsEngine,
  setVoicevoxStyle,
  type TtsEngine,
  type VoicevoxSpeaker,
  voicevoxInstalled,
  voicevoxSpeakers,
  ttsReady as fetchTtsReady,
  type SttEngine,
  setWakeConfig,
  setDebugMode,
  setShellAccess,
  setWebAccess,
  ttsKeyRegistered,
  voicePhrasesStatus,
} from "@/lib/api";

type KeyRowProps = {
  title: string;
  description?: string;
  registered: boolean;
  busy: boolean;
  onSave: (key: string) => Promise<void>;
  onDelete: () => Promise<void>;
};

/** 1行のコンパクトな接続モジュール。キー入力は「接続」を押すまで隠しておく。 */
function KeyRow({ title, description, registered, busy, onSave, onDelete }: KeyRowProps) {
  const t = useT();
  const [value, setValue] = useState("");
  const [open, setOpen] = useState(false);
  return (
    <div className="py-3">
      <div className="flex items-center gap-3">
        <span
          aria-hidden
          className={cn(
            "size-2 shrink-0 rounded-full border",
            registered ? "border-primary bg-primary shadow-[0_0_8px_var(--primary)]" : "border-muted-foreground/50",
          )}
        />
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">{title}</div>
          {description && <div className="text-xs text-muted-foreground">{description}</div>}
        </div>
        <span className="hud-label hidden text-[0.6rem] sm:inline" aria-live="polite">
          {registered ? "linked" : ""}
        </span>
        {registered && (
          <Button type="button" variant="ghost" size="icon-sm" aria-label={t("削除")} disabled={busy} onClick={onDelete}>
            <Trash2 />
          </Button>
        )}
        <Button type="button" variant="outline" size="sm" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          {registered ? t("更新") : t("接続")}
          <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
        </Button>
      </div>
      {open && (
        <form
          className="mt-3 flex gap-2 pl-5"
          onSubmit={async (e) => {
            e.preventDefault();
            await onSave(value);
            setValue("");
            setOpen(false);
          }}
        >
          <Input
            autoFocus
            type="password"
            autoComplete="off"
            placeholder={t("APIキーを貼り付け")}
            value={value}
            onChange={(e) => setValue(e.target.value)}
          />
          <Button type="submit" disabled={busy || !value.trim()}>
            {busy ? <Loader2 className="animate-spin" /> : <KeyRound />}
            {busy ? t("確認中…") : t("検証して保存")}
          </Button>
        </form>
      )}
    </div>
  );
}

/** セクション見出し。名前の横の「?」から詳しい説明を開く。 */
function Heading({ title, help, children }: { title: string; help: string; children: React.ReactNode }) {
  return (
    <div className="mb-1 flex items-center gap-2">
      <h2 className="hud-label">{title}</h2>
      <HelpDrawer title={help}>{children}</HelpDrawer>
    </div>
  );
}

type TuningKey = keyof Tuning;
type TuningField = { key: TuningKey; label: string; hint: string; unit: string; min: number; max: number; step: number };

/** 数値の調整項目の入力欄。入力欄を離れたときに自動で保存する。保存時は他のセクションが保存した項目を上書きしないよう、最新を読み直して自分の項目だけ差し替える。 */
function TuningPanel({ title, help, helpBody, fields }: { title: string; help: string; helpBody: React.ReactNode; fields: TuningField[] }) {
  const t = useT();
  const [saved, setSaved] = useState<Tuning | null>(null);
  const [draft, setDraft] = useState<Partial<Record<TuningKey, string>>>({});
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    getTuning()
      .then(setSaved)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const valueOf = (f: TuningField) => draft[f.key] ?? String(saved?.[f.key] ?? DEFAULT_TUNING[f.key]);
  const dirty = fields.some((f) => draft[f.key] !== undefined && Number(draft[f.key]) !== saved?.[f.key]);

  const save = async (reset: boolean) => {
    setBusy(true);
    try {
      const latest = await getTuning();
      const next = { ...latest };
      for (const f of fields) {
        const n = reset ? DEFAULT_TUNING[f.key] : Number(valueOf(f));
        if (Number.isFinite(n)) next[f.key] = n;
      }
      const stored = await setTuning(next);
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
    <section className="hud-panel px-5 py-4">
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

const VOICE_FIELDS: TuningField[] = [
  { key: "voice_idle_secs", label: "無音で入力を終える時間", hint: "声が入らないとき、音声入力を終えるまでの時間。", unit: "秒", min: 1, max: 30, step: 0.5 },
  { key: "utterance_silence_secs", label: "話し終わりの無音", hint: "会話の続きの待ち受けで、この無音が続くと話し終えたとみなす。", unit: "秒", min: 0.5, max: 5, step: 0.1 },
  { key: "release_tail_secs", label: "スペースを離したあとの保持", hint: "キーを離してから録音を止めるまで。語尾が切れるなら長くする。", unit: "秒", min: 0, max: 3, step: 0.5 },
];

const AGENT_FIELDS: TuningField[] = [
  { key: "web_search_max", label: "Web検索の回数", hint: "1回の依頼で検索できる上限。多いほど料金が増える。", unit: "回", min: 1, max: 20, step: 1 },
  { key: "web_fetch_max", label: "ページ取得の回数", hint: "1回の依頼でページを読める上限。", unit: "回", min: 1, max: 20, step: 1 },
  { key: "history_exchanges", label: "そのまま渡す会話の往復数", hint: "超えた古い会話はAIで要約して渡す(要約にも少量のトークンを使う)。", unit: "往復", min: 2, max: 30, step: 1 },
];

/** インターネット検索のオン/オフ。検索はAnthropic側で実行され、検索回数に応じた料金がかかる。 */
function WebAccess() {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    getWebAccess()
      .then(setEnabled)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const change = async (next: boolean) => {
    setEnabled(next);
    try {
      await setWebAccess(next);
    } catch (e) {
      setEnabled(!next);
      toast.error(reportError(e));
    }
  };

  return (
    <section className="hud-panel flex items-center justify-between gap-4 px-5 py-4">
      <div>
        <Heading title="Web Search" help={t("インターネット検索")}>
          <p>{t("調査の依頼などで、AIがインターネットを検索してページを読めるようにします。")}</p>
          <ul>
            <li>{t("Claudeを選んでいるときのみ使われます。")}</li>
            <li>{t("検索はAnthropic側で実行され、検索1回ごとに追加料金(1,000回あたり10ドル)がかかります。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("AIがネットを検索して調べものをします(検索ごとに追加料金)。")}</p>
      </div>
      <Switch
        checked={enabled ?? false}
        disabled={enabled === null}
        onCheckedChange={change}
        aria-label={t("インターネット検索")}
      />
    </section>
  );
}

/** シェルコマンドによる作業のオン/オフ。実行前の計画の承認と、コマンドごとの許可・拒否のルールで守られる。 */
function ShellAccess() {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    getShellAccess()
      .then(setEnabled)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const change = async (next: boolean) => {
    setEnabled(next);
    try {
      await setShellAccess(next);
    } catch (e) {
      setEnabled(!next);
      toast.error(reportError(e));
    }
  };

  return (
    <section className="hud-panel flex items-center justify-between gap-4 px-5 py-4">
      <div>
        <Heading title="Command Tasks" help={t("コマンドによる作業")}>
          <p>{t("「〜してください」という依頼に対し、AIが手順を提示し、あなたが音声で「はい」と承認すると、コマンドを実行して作業を進めます。")}</p>
          <ul>
            <li>{t("Claudeを選んでいるときのみ使われます。")}</li>
            <li>{t("rm・sudo・OSの設定に影響するコマンドは常に拒否します。")}</li>
            <li>{t("許可・拒否・都度承認のコマンドは、下の Command Policy で編集できます。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("依頼を承認すると、AIがコマンドを実行して作業します。")}</p>
      </div>
      <Switch
        checked={enabled ?? false}
        disabled={enabled === null}
        onCheckedChange={change}
        aria-label={t("コマンドによる作業")}
      />
    </section>
  );
}

type ListKey = "allow" | "ask" | "deny" | "deny_words" | "protected" | "system_paths" | "system_exceptions";
type FlagKey = "confirm_overwrite" | "confirm_pipe_to_shell" | "restrict_to_knowledge";

const POLICY_FIELDS: { key: ListKey; title: string; hint: string }[] = [
  { key: "allow", title: "承認なしで実行", hint: "コマンド名と引数の先頭が一致したときだけ有効(例: git status)" },
  { key: "ask", title: "毎回承認", hint: "引数の途中にあっても一致(例: npm publish)" },
  { key: "deny", title: "実行を拒否", hint: "引数の途中にあっても一致。最優先(例: defaults write)" },
  { key: "deny_words", title: "拒否する語(位置を問わない)", hint: "xargs rm や git rm のように、どこに現れても拒否" },
  { key: "protected", title: "機密データ", hint: "引数にこの文字列を含むコマンドを拒否(APIキー等のファイル名)" },
  { key: "system_paths", title: "システム領域", hint: "ここへの書き込み・リダイレクトを拒否(接頭辞)" },
  { key: "system_exceptions", title: "システム領域の例外", hint: "上のうち書き込みを許す接頭辞(例: /usr/local)" },
];

const POLICY_FLAGS: { key: FlagKey; title: string; hint: string }[] = [
  { key: "confirm_overwrite", title: "上書きリダイレクトを毎回承認", hint: "> でファイルを上書きするとき" },
  { key: "confirm_pipe_to_shell", title: "パイプ先のコード実行を毎回承認", hint: "curl … | sh のとき" },
  { key: "restrict_to_knowledge", title: "読み取り系以外はナレッジ内に限る", hint: "作業ディレクトリとパスがナレッジの外なら拒否" },
];

const toLines = (list: string[]) => list.join("\n");
const fromLines = (text: string) => text.split("\n");
const cleanLines = (text: string) => text.trim().replace(/\n{2,}/g, "\n");

/** コマンド権限の編集。1行1項目。入力欄を離れると自動で保存し、次のコマンドから使われる。rm・sudo・機密の拒否なども初期値として入っており、すべて編集できる。 */
function CommandPolicyEditor() {
  const t = useT();
  const [saved, setSaved] = useState<CommandPolicy | null>(null);
  const [draft, setDraft] = useState<CommandPolicy | null>(null);
  const [text, setText] = useState<Record<ListKey, string>>(() =>
    Object.fromEntries(POLICY_FIELDS.map(({ key }) => [key, ""])) as Record<ListKey, string>,
  );
  const [busy, setBusy] = useState(false);

  const apply = (p: CommandPolicy) => {
    setSaved(p);
    setDraft(p);
    setText(Object.fromEntries(POLICY_FIELDS.map(({ key }) => [key, toLines(p[key])])) as Record<ListKey, string>);
  };
  useEffect(() => {
    getCommandPolicy()
      .then(apply)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const dirty =
    saved !== null &&
    draft !== null &&
    (POLICY_FIELDS.some(({ key }) => toLines(saved[key]) !== cleanLines(text[key])) ||
      POLICY_FLAGS.some(({ key }) => saved[key] !== draft[key]));

  const current = (): CommandPolicy | null =>
    draft && {
      ...draft,
      ...(Object.fromEntries(POLICY_FIELDS.map(({ key }) => [key, fromLines(text[key])])) as Record<ListKey, string[]>),
    };

  /** 保存する。入力中の欄を上書きしないよう、画面の入力は触らず保存済みの値だけ更新する。 */
  const persist = async (policy: CommandPolicy | null) => {
    if (!policy) return;
    setBusy(true);
    try {
      setSaved(await setCommandPolicy(policy));
      toast.success(t("コマンド権限を保存しました"));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  /** 入力欄を離れたとき、変更があれば保存する。 */
  const saveIfDirty = () => dirty && !busy && persist(current());

  /** 初期値を読み込んで保存する。 */
  const loadDefaults = async () => {
    try {
      const d = await getDefaultCommandPolicy();
      setDraft(d);
      setText(Object.fromEntries(POLICY_FIELDS.map(({ key }) => [key, toLines(d[key])])) as Record<ListKey, string>);
      await persist(d);
    } catch (e) {
      toast.error(reportError(e));
    }
  };

  return (
    <section className="hud-panel space-y-3 px-5 py-4">
      <Heading title="Command Policy" help={t("コマンドの権限")}>
        <p>{t("コマンドごとの実行ルールです。1行に1つ、コマンド名と引数を書きます。")}</p>
        <ul>
          <li><strong>*</strong> {t("が使えます。")}<strong>aws s3 *</strong> {t("は aws s3 以下すべて、")}<strong>aws * delete-*</strong> {t("は語の中や途中の指定です。")}</li>
          <li>{t("rm・sudo・OSの設定に影響するコマンド、APIキーのファイルの拒否なども、初期値として入っているだけで、すべて編集できます。消すとその保護は無くなります。")}</li>
          <li>{t("「初期値に戻す」で初期値に戻せます。")}</li>
          <li>{t("ナレッジ内の")} <strong>.tachicoma/command-policy.json</strong> {t("に保存されます。")}</li>
        </ul>
      </Heading>
      <div className="grid gap-3 md:grid-cols-3">
        {POLICY_FIELDS.map(({ key, title, hint }) => (
          <label key={key} className="space-y-1 text-xs">
            <span className="font-medium text-foreground">{t(title)}</span>
            <Textarea
              value={text[key]}
              disabled={saved === null}
              rows={7}
              spellCheck={false}
              className="font-mono text-xs"
              onChange={(e) => setText((t) => ({ ...t, [key]: e.target.value }))}
              onBlur={saveIfDirty}
            />
            <span className="block text-muted-foreground">{t(hint)}</span>
          </label>
        ))}
      </div>
      <div className="space-y-2">
        {POLICY_FLAGS.map(({ key, title, hint }) => (
          <div key={key} className="flex items-center justify-between gap-3 text-xs">
            <div>
              <p className="font-medium text-foreground">{t(title)}</p>
              <p className="text-muted-foreground">{t(hint)}</p>
            </div>
            <Switch
              checked={draft?.[key] ?? false}
              disabled={draft === null}
              onCheckedChange={(v) => {
                const policy = current();
                if (!policy) return;
                setDraft({ ...draft!, [key]: v });
                persist({ ...policy, [key]: v });
              }}
              aria-label={title}
            />
          </div>
        ))}
      </div>
      <div className="flex justify-end gap-2">
        <Button variant="outline" disabled={saved === null || busy} onClick={loadDefaults}>
          {busy && <Loader2 className="animate-spin" />} {t("初期値に戻す")}
        </Button>
      </div>
    </section>
  );
}

const BATCH_INTERVALS = [5, 15, 30, 60, 180, 360, 720, 1440];
const intervalLabel = (m: number) => (m < 60 ? tr("{m}分", { m }) : tr("{m}時間", { m: m / 60 }));

/** ログ整理(ナレッジ化)を自動実行する間隔。 */
function BatchInterval() {
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
    <section className="hud-panel flex items-center justify-between gap-4 px-5 py-4">
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

/** 表示言語。選択はこの端末に保存され、トレイメニューにも反映される。 */
function LanguageSetting() {
  const t = useT();
  const lang = useLang();
  return (
    <section className="hud-panel flex items-center justify-between gap-4 px-5 py-4">
      <div>
        <Heading title="Language" help={t("表示言語")}>
          <p>{t("画面とメニューバーの表示言語を切り替えます。")}</p>
          <ul>
            <li>{t("AIの応答や音声認識の言語は変わりません。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("画面とメニューバーの表示言語")}</p>
      </div>
      <Select items={LANGS} value={lang} onValueChange={(v) => v && setLang(v as Lang)}>
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

/** デバッグモードのオン/オフ。ログにはAPIへ送る会話内容や文字起こしがそのまま残る。 */
function DebugMode() {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    getDebugMode()
      .then(setEnabled)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const change = async (next: boolean) => {
    setEnabled(next);
    try {
      await setDebugMode(next);
      window.dispatchEvent(new Event(DEBUG_MODE_EVENT));
    } catch (e) {
      setEnabled(!next);
      toast.error(reportError(e));
    }
  };

  return (
    <section className="hud-panel flex items-center justify-between gap-4 px-5 py-4">
      <div>
        <Heading title="Debug Mode" help={t("デバッグログの記録")}>
          <p>{t("各APIへのリクエスト・レスポンスと、音声入力の文字起こし結果をファイルに記録します。")}</p>
          <ul>
            <li>{t("ナレッジ内の")} <strong>debug-logs</strong> {t("フォルダに、日付ごとの")} <strong>.jsonl</strong> {t("ファイルとして追記されます。")}</li>
            <li>{t("会話の内容がそのまま残ります。不要になったらオフにして、ファイルを削除してください。")}</li>
            <li>{t("APIキーと音声データ本体は記録しません。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("API通信と文字起こしをナレッジ内の debug-logs に記録します。")}</p>
      </div>
      <Switch
        checked={enabled ?? false}
        disabled={enabled === null}
        onCheckedChange={change}
        aria-label={t("デバッグモード")}
      />
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
function PersonalData() {
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
    <section className="hud-panel px-5 py-4">
      <Heading title="Personal Data" help={t("パーソナルデータ")}>
        <p>{t("あなたの基本情報をAIへ毎回渡し、呼びかけや回答を合わせてもらいます。")}</p>
        <ul>
          <li>{t("入力した項目だけが、選んでいるAIのプロバイダ(AnthropicまたはOpenAI)へ送られます。")}</li>
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
function BaseInstructions() {
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
    <section className="hud-panel px-5 py-4">
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
function Proofread({ keyStatus }: { keyStatus: Partial<KeyStatus> }) {
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
    <section className="hud-panel px-5 py-4">
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

/** 常時音声待機。起動ワードを含む発話があったときだけ、依頼の待ち受けに入る。 */
/** 繰り返し使う定型の音声を作成するボタン。作成済みは再送しない。実行時はTTSへ送らず、保存した音声を再生する。 */
function VoicePhrases({ ready, engine, style }: { ready: boolean; engine: TtsEngine; style: number }) {
  const t = useT();
  const [phrases, setPhrases] = useState<VoicePhrase[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    voicePhrasesStatus()
      .then(setPhrases)
      .catch((e) => toast.error(reportError(e)));
  }, [engine, style]); // 声ごとに別の音声として保存されるため、切り替えたら作成状況を取り直す

  const missing = phrases.some((p) => !p.ready);
  const create = async () => {
    setBusy(true);
    try {
      setPhrases(await generateVoicePhrases());
      toast.success(t("定型音声を作成しました。"));
    } catch (e) {
      toast.error(reportError(e));
      voicePhrasesStatus().then(setPhrases).catch(() => {});
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="border-t py-3">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">{t("定型音声")}</div>
          <div className="text-xs text-muted-foreground">
            {t("呼びかけへの応答や会話切り替えの確認で繰り返し使う音声を、先に作成して保存します。")}
            {ready ? "" : engine === "voicevox" ? t("作成にはVOICEVOXの取得が必要です。") : t("作成にはGoogle Cloudのキーが必要です。")}
          </div>
        </div>
        <Button size="sm" variant="outline" disabled={!ready || busy || !missing} onClick={create}>
          {busy ? <Loader2 className="animate-spin" /> : null}
          {busy ? t("作成中…") : missing ? t("定型音声を作成") : t("作成済み")}
        </Button>
      </div>
      <ul className="mt-2 space-y-1 text-xs">
        {phrases.map((p) => (
          <li key={p.id} className="flex items-center gap-2">
            <Badge variant={p.ready ? "default" : "outline"}>{p.ready ? t("作成済み") : t("未作成")}</Badge>
            <span className="text-muted-foreground">{p.text}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 読み上げ前に置き換える語と読み。英単語やサービス名がアルファベット読みされるのを、カタカナで直す。字幕は元の表記のまま。 */
function PronunciationDictionary() {
  const t = useT();
  const [dict, setDict] = useState<TtsDictionary | null>(null);
  const [word, setWord] = useState("");
  const [reading, setReading] = useState("");
  const [busy, setBusy] = useState(false);
  // 編集中の行(編集前の語)と、その入力値
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<DictionaryEntry>({ word: "", reading: "" });
  useEffect(() => {
    getTtsDictionary()
      .then(setDict)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const save = async (user: DictionaryEntry[]) => {
    setBusy(true);
    try {
      const saved = await setTtsDictionary(user);
      setDict((d) => d && { ...d, user: saved });
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  // 同じ語(大文字小文字違いを含む)を登録し直すと、読みが上書きされる
  const add = async () => {
    if (!dict || !word.trim() || !reading.trim()) return;
    if (await save([...dict.user, { word, reading }])) {
      setWord("");
      setReading("");
    }
  };

  // 語を変えた場合は元の行を置き換える(別の登録済みの語に変えたときは、そちらが上書きされる)
  const commitEdit = async () => {
    if (!dict || editing === null || !draft.word.trim() || !draft.reading.trim()) return;
    const rest = dict.user.filter((x) => x.word !== editing);
    if (await save([...rest, draft])) setEditing(null);
  };

  return (
    <div className="border-t py-3">
      <div className="text-sm font-medium">{t("読み上げ辞書")}</div>
      <div className="text-xs text-muted-foreground">
        {t("英単語やサービス名がアルファベットで読まれるときは、読みをカタカナで登録します。字幕は元の表記のままです。大文字小文字は区別せず、前後が英数字でつながる語(GitHubberなど)には適用しません。")}
      </div>
      <form
        className="mt-2 flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <Input placeholder={t("語(例: Figma)")} aria-label={t("語")} maxLength={64} value={word} onChange={(e) => setWord(e.target.value)} />
        <Input placeholder={t("読み(例: フィグマ)")} aria-label={t("読み")} maxLength={128} value={reading} onChange={(e) => setReading(e.target.value)} />
        <Button type="submit" variant="outline" disabled={!dict || busy || !word.trim() || !reading.trim()}>
          <Plus /> {t("追加")}
        </Button>
      </form>
      {!!dict?.user.length && (
        <ul className="mt-2 space-y-1 text-sm">
          {dict.user.map((e) =>
            editing === e.word ? (
              <li key={e.word}>
                <form
                  className="flex items-center gap-2"
                  onSubmit={(ev) => {
                    ev.preventDefault();
                    void commitEdit();
                  }}
                >
                  <Input aria-label={t("語")} maxLength={64} value={draft.word} onChange={(ev) => setDraft({ ...draft, word: ev.target.value })} />
                  <Input aria-label={t("読み")} maxLength={128} value={draft.reading} onChange={(ev) => setDraft({ ...draft, reading: ev.target.value })} />
                  <Button type="submit" variant="ghost" size="icon-xs" aria-label={t("保存")} disabled={busy || !draft.word.trim() || !draft.reading.trim()}>
                    <Check />
                  </Button>
                  <Button type="button" variant="ghost" size="icon-xs" aria-label={t("キャンセル")} disabled={busy} onClick={() => setEditing(null)}>
                    <X />
                  </Button>
                </form>
              </li>
            ) : (
              <li key={e.word} className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate">
                  {e.word} <span className="text-muted-foreground">→ {e.reading}</span>
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={t("{w}を編集", { w: e.word })}
                  disabled={busy}
                  onClick={() => {
                    setEditing(e.word);
                    setDraft(e);
                  }}
                >
                  <Pencil />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={t("{w}を削除", { w: e.word })}
                  disabled={busy}
                  onClick={() => {
                    if (editing === e.word) setEditing(null);
                    void save(dict.user.filter((x) => x.word !== e.word));
                  }}
                >
                  <X />
                </Button>
              </li>
            ),
          )}
        </ul>
      )}
      {!!dict?.builtin.length && (
        <details className="mt-2 text-xs text-muted-foreground">
          <summary className="cursor-pointer">{t("組み込みの読み({n}語)", { n: dict.builtin.length })}</summary>
          <p className="mt-1">{t("同じ語を上で登録すると、そちらが優先されます。")}</p>
          <ul className="mt-1 flex flex-wrap gap-x-4 gap-y-0.5">
            {dict.builtin.map((e) => (
              <li key={e.word}>
                {e.word} → {e.reading}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

function WakeWords({ ttsReady, engine, cloudStt }: { ttsReady: boolean; engine: TtsEngine; cloudStt: string | null }) {
  const t = useT();
  const [config, setConfig] = useState<WakeConfig | null>(null);
  const [word, setWord] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    getWakeConfig()
      .then(setConfig)
      .catch((e) => toast.error(reportError(e)));
  }, []);

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

  return (
    <section className="hud-panel px-5 py-4">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <Heading title="Wake Words" help={t("常時音声待機")}>
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

/** 設定の書き出し・読み込み。APIキーは含まれないので、移行先で登録し直す。 */
function SettingsTransfer() {
  const t = useT();
  const [busy, setBusy] = useState(false);

  const exportAll = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const dest = await saveDialog({
      title: t("設定のエクスポート"),
      defaultPath: `myassistant-settings-${stamp}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!dest) return;
    setBusy(true);
    try {
      const r = await exportSettings(dest);
      toast.success(t("{applied}項目を書き出しました。", { applied: r.applied }));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  const importAll = async () => {
    const src = await openDialog({
      title: t("設定のインポート"),
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof src !== "string") return;
    setBusy(true);
    try {
      const r = await importSettings(src);
      toast.success(t("{applied}項目を読み込みました。", { applied: r.applied }));
      // 各項目が保持している画面上の値を、読み込んだ内容に合わせ直す
      setTimeout(() => window.location.reload(), 600);
    } catch (e) {
      toast.error(reportError(e));
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel px-5 py-4">
      <Heading title="Backup" help={t("設定の移行")}>
        <p>{t("AIの動作、モデル選択、音声、呼びかけ、コマンド権限などの設定をJSONファイルで書き出し・読み込みできます。")}</p>
        <ul>
          <li>{t("APIキーは含まれません。移行先で登録し直してください。")}</li>
          <li>{t("ナレッジの保存先と会話の履歴も含まれません。")}</li>
          <li>{t("読み込むと、ファイルにある項目が現在の設定を上書きします。コマンド権限も置き換わるので、信頼できるファイルだけを読み込んでください。")}</li>
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

/** VOICEVOX COREの取得と声の選択。COREは同梱せず、公式の配布物をアプリデータへ取得して使う。 */
function VoicevoxPanel({ installed, style, onChanged, onStyle }: { installed: boolean; style: number; onChanged: () => void; onStyle: (id: number) => void }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [speakers, setSpeakers] = useState<VoicevoxSpeaker[]>([]);
  useEffect(() => {
    if (!installed) return;
    voicevoxSpeakers()
      .then(setSpeakers)
      .catch((e) => toast.error(reportError(e)));
  }, [installed]);

  const install = async () => {
    setBusy(true);
    try {
      await installVoicevox();
      toast.success(t("VOICEVOXを取得しました。"));
      onChanged();
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="border-t py-3">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-sm font-medium">
            VOICEVOX CORE
            <Badge variant={installed ? "default" : "outline"}>{installed ? t("取得済み") : t("未取得")}</Badge>
          </div>
          <div className="text-xs text-muted-foreground">
            {t("端末内で音声を合成します(キー不要・無料)。公式配布のCORE・ONNX Runtime・辞書・音声モデル(約90MB)をダウンロードします。")}
          </div>
        </div>
        <Button size="sm" variant="outline" disabled={busy} onClick={install}>
          {busy ? <Loader2 className="animate-spin" /> : <Download />}
          {busy ? t("取得中…") : installed ? t("取得し直す") : t("規約に同意して取得")}
        </Button>
      </div>
      <p className="mt-2 text-xs text-muted-foreground">
        {t("利用規約(github.com/VOICEVOX/voicevox_vvm)と各音声ライブラリの規約に従ってください。音声を公開・配布するときは「VOICEVOX:ずんだもん」のようにクレジットの表記が必要です。")}
      </p>
      {installed && speakers.length > 0 && (
        <div className="mt-3 flex items-center gap-3">
          <div className="min-w-0 flex-1 text-sm font-medium">{t("声")}</div>
          <select
            aria-label={t("声")}
            className="h-9 rounded-md border bg-background px-2 text-sm"
            value={style}
            onChange={(e) => onStyle(Number(e.target.value))}
          >
            {speakers.map((sp) => (
              <optgroup key={sp.name} label={sp.name}>
                {sp.styles.map((st) => (
                  <option key={st.id} value={st.id}>
                    {sp.name}({st.name})
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </div>
      )}
    </div>
  );
}

export default function Setup() {
  const t = useT();
  const router = useRouter();
  const [status, setStatus] = useState<Partial<KeyStatus>>({});
  const [ttsReady, setTtsReady] = useState(false); // Google Cloudのキー(読み上げ・Google音声認識で共通)
  const [speechReady, setSpeechReady] = useState(false); // 選んだ読み上げエンジンが使える状態か
  const [ttsEngine, setTtsEngineState] = useState<TtsEngine>("google");
  const [vvInstalled, setVvInstalled] = useState(false);
  const [vvStyle, setVvStyle] = useState(3);
  const [aquaReady, setAquaReady] = useState(false);
  const [sttEngine, setSttEngineState] = useState<SttEngine>("local");
  const [sttLang, setSttLang] = useState<SttLanguage>("auto");
  const [sttUpgradable, setSttUpgradable] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      Promise.all([getKeyStatus(), ttsKeyRegistered(), aquaKeyRegistered(), getSttEngine(), getSttLanguage(), sttModelUpgradable(), getTtsEngine(), voicevoxInstalled(), getVoicevoxStyle(), fetchTtsReady()])
        .then(([s, t, a, e, l, u, te, vi, vs, sr]) => {
          setTtsEngineState(te);
          setVvInstalled(vi);
          setVvStyle(vs);
          setSpeechReady(sr);
          setSttLang(l);
          setSttUpgradable(u);
          setStatus(s);
          setTtsReady(t);
          setAquaReady(a);
          setSttEngineState(e);
        })
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);

  const run = async (id: string, fn: () => Promise<void>, ok?: string) => {
    setBusy(id);
    try {
      await fn();
      await refresh();
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(null);
    }
  };

  const hasAny = Object.values(status).some(Boolean);
  const linked = PROVIDERS.filter((p) => status[p]).length;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
        <div>
          <p className="hud-label mb-1">{"// System config"}</p>
          <h1 className="text-2xl font-semibold tracking-tight">{t("設定")}</h1>
        </div>

        <Tabs defaultValue="ai" className="gap-6">
          <TabsList variant="line" className="w-full justify-start overflow-x-auto border-b pb-1">
            <TabsTrigger value="ai" className="flex-none px-3">{t("AI接続")}</TabsTrigger>
            <TabsTrigger value="voice" className="flex-none px-3">{t("音声")}</TabsTrigger>
            <TabsTrigger value="behavior" className="flex-none px-3">{t("AIの動作")}</TabsTrigger>
            <TabsTrigger value="access" className="flex-none px-3">{t("権限")}</TabsTrigger>
            <TabsTrigger value="data" className="flex-none px-3">{t("その他")}</TabsTrigger>
          </TabsList>

          <TabsContent value="ai" className="flex flex-col gap-6">
          <section className="hud-panel px-5 py-4">
            <div className="mb-1 flex items-center justify-between">
              <Heading title="AI Link" help={t("AIの接続")}>
                <p>{t("使いたいAIのAPIキーを登録します。全部は不要で、1つ以上あれば使えます。")}</p>
                <ul>
                  <li>{t("キーは疎通確認に成功した場合のみ、この端末のアプリデータ内に保存されます。")}</li>
                  <li>{t("保存先は平文のJSONファイルです(OSのキーチェーンは使っていません)。")}</li>
                </ul>
              </Heading>
              <span className="hud-label text-muted-foreground">{linked} / {PROVIDERS.length}</span>
            </div>
            <div className="divide-y divide-border">
              {PROVIDERS.map((p: Provider) => (
                <KeyRow
                  key={p}
                  title={PROVIDER_LABELS[p]}
                  registered={!!status[p]}
                  busy={busy === p}
                  onSave={(k) => run(p, () => saveApiKey(p, k), t("APIキーを保存しました。"))}
                  onDelete={() => run(p, () => deleteApiKey(p))}
                />
              ))}
            </div>
          </section>

          </TabsContent>

          <TabsContent value="voice" className="flex flex-col gap-6">
          <section className="hud-panel px-5 py-1">
            <div className="pt-3">
              <Heading title="Voice Link" help={t("Google Cloudの音声機能")}>
                <p>{t("応答の読み上げ(Text-to-Speech)と、Google音声認識(Speech-to-Text)で共通のキーです。")}</p>
                <ul>
                  <li>{t("Google Cloud ConsoleでCloud Text-to-Speech API(認識も使うならCloud Speech-to-Text API)の有効化が必要です。")}</li>
                  <li>{t("音声認識のエンジンがローカルの間は、Whisperで認識します(無料)。Aqua Voiceは音声認識のみで、読み上げ(TTS)には対応していません。")}</li>
                  <li>{t("音声認識にGoogleを選ぶと録音音声がGoogleへ送信され、課金対象になります。")}</li>
                </ul>
              </Heading>
            </div>
            <KeyRow
              title="Google Cloud"
              description={t("読み上げ・音声認識用のAPIキー")}
              registered={ttsReady}
              busy={busy === "tts"}
              onSave={(k) => run("tts", () => saveTtsKey(k), t("APIキーを保存しました。"))}
              onDelete={() =>
                run("tts", async () => {
                  await deleteTtsKey();
                  if (sttEngine === "google") await setSttEngine("local"); // キーがないGoogle認識は使えない
                })
              }
            />
            <VoicevoxPanel
              installed={vvInstalled}
              style={vvStyle}
              onChanged={refresh}
              onStyle={(id) => run("vv-style", () => setVoicevoxStyle(id))}
            />
            <div className="flex items-center gap-3 border-t py-3">
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{t("読み上げエンジン")}</div>
                <div className="text-xs text-muted-foreground">{t("Google Cloudはクラウドで合成し、課金対象です。VOICEVOXは端末内で合成し、無料です。")}</div>
              </div>
              <select
                aria-label={t("読み上げエンジン")}
                className="h-9 rounded-md border bg-background px-2 text-sm"
                value={ttsEngine}
                disabled={busy === "tts-engine"}
                onChange={(e) => {
                  const engine = e.target.value as TtsEngine;
                  run("tts-engine", () => setTtsEngine(engine));
                }}
              >
                <option value="google">Google Cloud</option>
                <option value="voicevox">VOICEVOX</option>
              </select>
            </div>
            <VoicePhrases ready={speechReady} engine={ttsEngine} style={vvStyle} />
            <PronunciationDictionary />
            <KeyRow
              title="Aqua Voice"
              description={t("音声認識用のAPIキー(読み上げには使えません)")}
              registered={aquaReady}
              busy={busy === "aqua"}
              onSave={(k) => run("aqua", () => saveAquaKey(k), t("APIキーを保存しました。"))}
              onDelete={() =>
                run("aqua", async () => {
                  await deleteAquaKey();
                  if (sttEngine === "aqua") await setSttEngine("local"); // キーがないAqua認識は使えない
                })
              }
            />
            <div className="flex items-center gap-3 border-t py-3">
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{t("音声認識エンジン")}</div>
                <div className="text-xs text-muted-foreground">{t("ローカルのWhisperは無料。GoogleとAquaは録音音声を送信し、課金対象です。")}</div>
              </div>
              <select
                aria-label={t("音声認識エンジン")}
                className="h-9 rounded-md border bg-background px-2 text-sm"
                value={sttEngine}
                disabled={busy === "stt"}
                onChange={(e) => {
                  const engine = e.target.value as SttEngine;
                  run("stt", () => setSttEngine(engine));
                }}
              >
                <option value="local">{t("ローカル (Whisper)")}</option>
                <option value="google" disabled={!ttsReady}>Google Cloud</option>
                <option value="aqua" disabled={!aquaReady}>Aqua Voice</option>
              </select>
            </div>
            {sttEngine === "local" && sttUpgradable && (
              <div className="flex items-center gap-3 border-t py-3">
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-medium">{t("高精度モデルに更新")}</div>
                  <div className="text-xs text-muted-foreground">{t("日本語の認識精度が上がります(約550MBのダウンロード)。更新するまで今のモデルを使い続けます。")}</div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={busy === "sttmodel"}
                  onClick={() => run("sttmodel", downloadSttModel, t("高精度モデルをインストールしました。"))}
                >
                  {busy === "sttmodel" ? <Loader2 className="animate-spin" /> : <Download />}
                  {busy === "sttmodel" ? t("ダウンロード中…") : t("更新")}
                </Button>
              </div>
            )}
            <div className="flex items-center gap-3 border-t py-3">
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{t("認識する言語")}</div>
                <div className="text-xs text-muted-foreground">{t("自動は日本語と英語を判定します。どのエンジンにも適用されます。")}</div>
              </div>
              <select
                aria-label={t("音声認識の言語")}
                className="h-9 rounded-md border bg-background px-2 text-sm"
                value={sttLang}
                disabled={busy === "sttlang"}
                onChange={(e) => {
                  const l = e.target.value as SttLanguage;
                  run("sttlang", async () => {
                    await setSttLanguage(l);
                    setSttLang(l);
                  });
                }}
              >
                <option value="auto">{t("自動")}</option>
                <option value="ja">{t("日本語")}</option>
                <option value="en">English</option>
                <option value="zh">{t("中文")}</option>
                <option value="ko">한국어</option>
              </select>
            </div>
          </section>

          <WakeWords ttsReady={speechReady} engine={ttsEngine} cloudStt={sttEngine === "google" ? "Google Speech-to-Text" : sttEngine === "aqua" ? "Aqua Voice" : null} />

          <TuningPanel
            title="Voice Timing"
            help={t("音声入力の待ち時間")}
            helpBody={
              <>
                <p>{t("録音を終えるタイミングを調整します。")}</p>
                <ul>
                  <li>{t("声が入らない時間が設定を超えると、音声入力を自動で終えます(手動の録音・返答への続き・起動ワードの後の用件のすべて)。")}</li>
                  <li>{t("次に対話画面を開いたときから反映されます。")}</li>
                </ul>
              </>
            }
            fields={VOICE_FIELDS}
          />

          <Proofread keyStatus={status} />
          </TabsContent>

          <TabsContent value="behavior" className="flex flex-col gap-6">
          <PersonalData />
          <BaseInstructions />
          <BatchInterval />
          <TuningPanel
            title="Limits"
            help={t("検索回数などの上限")}
            helpBody={
              <>
                <p>{t("AIが1回の依頼で使う量の上限です。")}</p>
                <ul>
                  <li>{t("Web検索とページ取得の回数は、Web Searchがオンのときだけ使われます。")}</li>
                  <li>{t("次の送信から反映されます。")}</li>
                </ul>
              </>
            }
            fields={AGENT_FIELDS}
          />
          </TabsContent>

          <TabsContent value="access" className="flex flex-col gap-6">
          <WebAccess />
          <ShellAccess />
          <CommandPolicyEditor />
          </TabsContent>

          <TabsContent value="data" className="flex flex-col gap-6">
          <LanguageSetting />
          <SettingsTransfer />
          <DebugMode />
          </TabsContent>
        </Tabs>

        <div className="flex justify-end">
          <Button disabled={!hasAny} className={cn(hasAny && "neon-glow")} onClick={() => router.replace("/")}>
            {t("完了して対話へ")} <ArrowRight />
          </Button>
        </div>
      </div>
    </div>
  );
}
