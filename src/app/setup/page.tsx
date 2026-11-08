"use client";
import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { Archive, ArrowRight, AudioLines, Brain, Check, ChevronDown, Download, KeyRound, Loader2, Pencil, Play, Plug, Plus, ShieldCheck, Square, Trash2, Upload, X } from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import AwsPanel from "@/components/AwsPanel";
import DeleteSpaceModal from "@/components/DeleteSpaceModal";
import NotePanel from "@/components/NotePanel";
import CopySettingsCard from "@/components/CopySettingsCard";
import CopyConfirmModal from "@/components/CopyConfirmModal";
import { copyTargets, type CopyRequest } from "@/lib/copyTargets";
import { DEFAULT_TUNING, getTuning, setTuning, LEADER_ID, PERSONAL_ID, getSpaceData, saveSpaceData, errorMessage, updateSpace, SPACE_NAME_MAX, leaderCopyApiKey, leaderGetPrompt, leaderSetPrompt, type DeleteReport } from "@/lib/api";
import { bumpSettingsVersion, refreshWorkspace, useWorkspace } from "@/lib/workspace";
import { cn } from "@/lib/utils";
import { PROVIDERS, PROVIDER_LABELS, type KeyStatus, type Provider, deleteApiKey, aquaKeyRegistered, deleteAquaKey, deleteTtsKey, exportSettings, importSettings, getKeyStatus, type Tuning, DEBUG_MODE_EVENT, type VoicePhrase, generateVoicePhrases, getCommandPolicy, getDefaultCommandPolicy, getDebugLogDir, getDebugMode, setCommandPolicy, type CommandPolicy, getShellAccess, getTtsDictionary, setTtsDictionary, exportTtsDictionary, importTtsDictionary, type DictionaryEntry, type TtsDictionary, getWebAccess, getSttEngine, getTtsEngine, getModelSources, getVoicevoxStyle, setVoicevoxVersions, setWhisperRevision, type ModelSources, type VoicevoxVersions, installVoicevox, getSttLanguage, downloadSttModel, sttModelUpgradable, setSttLanguage, type SttLanguage, reportError, saveApiKey, saveAquaKey, saveTtsKey, setSttEngine, setTtsEngine, setVoicevoxStyle, type TtsEngine, type VoicevoxSpeaker, voicevoxInstalled, voicevoxSpeakers, ttsReady as fetchTtsReady, type SttEngine, setDebugLogDir, setDebugMode, setShellAccess, setWebAccess, playBase64Mp3, stopSpeech, ttsKeyRegistered, voicePhraseAudio, voicePhrasesStatus, waitForSpeechEnd } from "@/lib/api";
import { Heading, PageHeader, Proofread, StatusPill, TuningPanel, type TuningField } from "@/components/GlobalPanels";

type KeyRowProps = {
  title: string;
  description?: string;
  registered: boolean;
  busy: boolean;
  /** カードとして出す(AI接続のプロバイダー一覧)。登録済みは縁が発光する。 */
  card?: boolean;
  onSave: (key: string) => Promise<void>;
  onDelete: () => Promise<void>;
};

/** 1行のコンパクトな接続モジュール。キー入力は「接続」を押すまで隠しておく。 */
function KeyRow({ title, description, registered, busy, card, onSave, onDelete }: KeyRowProps) {
  const t = useT();
  const [value, setValue] = useState("");
  const [open, setOpen] = useState(false);
  return (
    <div className={card ? cn("rounded-xl border px-4 py-3 transition-colors", registered ? "border-primary/40 bg-primary/5 shadow-[0_0_14px_oklch(0.85_0.14_180/12%)]" : "border-dashed") : "py-3"}>
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
        <span className="hidden sm:inline" aria-live="polite">
          {registered && <StatusPill on>linked</StatusPill>}
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

const AGENT_FIELDS: TuningField<Tuning>[] = [
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
    <section className="hud-panel hud-accent flex items-center justify-between gap-4 px-5 py-4">
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
    <section className="hud-panel hud-accent flex items-center justify-between gap-4 px-5 py-4">
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
    <section className="hud-panel hud-accent space-y-3 px-5 py-4">
      <Heading title="Command Policy" help={t("コマンドの権限")}>
        <p>{t("コマンドごとの実行ルールです。1行に1つ、コマンド名と引数を書きます。")}</p>
        <ul>
          <li><strong>*</strong> {t("が使えます。")}<strong>aws s3 *</strong> {t("は aws s3 以下すべて、")}<strong>aws * delete-*</strong> {t("は語の中や途中の指定です。")}</li>
          <li>{t("rm・sudo・OSの設定に影響するコマンド、APIキーのファイルの拒否なども、初期値として入っているだけで、すべて編集できます。消すとその保護は無くなります。")}</li>
          <li>{t("「初期値に戻す」で初期値に戻せます。")}</li>
          <li>{t("ナレッジ内の")} <strong>.my-assistant/command-policy.json</strong> {t("に保存されます。")}</li>
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

/** 選択中のスペースへ、パーソナルやほかのスペースの設定をコピーする。パーソナルには出さない。上書きの前に、確認のモーダルで必ず再確認する。 */
function CopyIntoSpace() {
  const t = useT();
  const { active, spaces } = useWorkspace();
  const [request, setRequest] = useState<CopyRequest | null>(null);
  if (active.id === PERSONAL_ID || active.id === LEADER_ID) return null; // リーダーは、設定のコピー先にしない
  const targets = copyTargets(spaces);
  return (
    <>
      <CopySettingsCard targets={targets} suggestedTo={null} fixedTo={active.id} onRequest={setRequest} />
      {request && (
        <CopyConfirmModal
          request={request}
          targets={targets}
          onClose={() => setRequest(null)}
          onCopied={(r) => {
            toast.success(t("設定を{count}件コピーしました。", { count: r.settings }) + (r.keys > 0 ? t("(APIキー{keys}件)", { keys: r.keys }) : "") + (r.words > 0 ? t("(読み上げ辞書{words}語)", { words: r.words }) : ""));
            bumpSettingsVersion(); // コピー先は選択中のスペースなので、設定画面を読み直す
          }}
        />
      )}
    </>
  );
}

/** 選択中のスペースの名称とイメージカラーの変更。パーソナルとリーダーには出さない(固定のため)。 */
function EditSpace() {
  const t = useT();
  const { active, spaces, palette } = useWorkspace();
  const space = spaces.find((p) => p.id === active.id);
  const [name, setName] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (active.id === PERSONAL_ID || active.id === LEADER_ID || !space) return null;

  // 未編集のあいだは、いまの値を表示する。ほかのスペースが使っている色は選べない
  const shownName = name ?? space.name;
  const color = picked ?? space.color;
  const usedBy = new Map(spaces.filter((p) => p.id !== space.id).map((p) => [p.color, p.name]));
  const dirty = shownName.trim() !== space.name || color !== space.color;
  const save = async () => {
    setBusy(true);
    try {
      await updateSpace(space.id, shownName, color);
      await refreshWorkspace();
      setName(null);
      setPicked(null);
      toast.success(t("スペースを更新しました。"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Space" help={t("スペースの名称と色")}>
        <p>{t("名称とイメージカラーは、どのエージェントと話しているかを見分けるために表示されます。名称と色はスペースごとに重複できず、「パーソナル」「ワーク」などは予約名です。")}</p>
      </Heading>
      <div className="flex flex-col gap-4">
        <Input
          value={shownName}
          maxLength={SPACE_NAME_MAX}
          placeholder={t("スペース名")}
          aria-label={t("スペース名")}
          onChange={(e) => setName(e.target.value)}
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
        <div>
          <Button onClick={() => void save()} disabled={busy || !dirty || shownName.trim().length === 0}>
            {busy ? <Loader2 className="animate-spin" /> : <Check />}
            {t("保存")}
          </Button>
        </div>
      </div>
    </section>
  );
}

/** 選択中のスペースの削除。パーソナルには出さない。削除の前に、確認のモーダルで必ず再確認する。 */
function DeleteSpace() {
  const t = useT();
  const router = useRouter();
  const { active, spaces } = useWorkspace();
  const [confirming, setConfirming] = useState(false);
  const space = spaces.find((p) => p.id === active.id);
  if (active.id === PERSONAL_ID || !space) return null;

  const deleted = async (r: DeleteReport) => {
    await refreshWorkspace();
    toast.success(t("スペースを削除しました。"), r.keptKnowledge ? { description: t("ナレッジは次の場所に残しました: {path}", { path: r.keptKnowledge }) } : undefined);
    router.replace("/"); // 削除したスペースの画面から、パーソナルへ戻る
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Danger Zone" help={t("スペースの削除")}>
        <p>{t("このスペースを、会話・設定・APIキーごと削除します。元に戻せません。")}</p>
        <ul>
          <li>{t("ナレッジは、削除時に残すか消すかを選べます(既定は残す)。")}</li>
          <li>{t("削除すると、パーソナルへ戻ります。")}</li>
        </ul>
      </Heading>
      <div className="mt-3 flex items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">{t("「{name}」を削除します。", { name: space.name })}</p>
        <Button variant="destructive" size="sm" onClick={() => setConfirming(true)}>
          <Trash2 />
          {t("このスペースを削除…")}
        </Button>
      </div>
      {confirming && <DeleteSpaceModal space={space} onClose={() => setConfirming(false)} onDeleted={(r) => void deleted(r)} />}
    </section>
  );
}

/** デバッグモードのオン/オフ。ログにはAPIへ送る会話内容や文字起こしがそのまま残る。 */
function DebugMode() {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [dir, setDir] = useState("");
  useEffect(() => {
    getDebugMode()
      .then(setEnabled)
      .catch((e) => toast.error(reportError(e)));
    getDebugLogDir()
      .then(setDir)
      .catch((e) => toast.error(reportError(e)));
  }, []);

  const changeDir = async (path: string | null) => {
    try {
      setDir(await setDebugLogDir(path));
    } catch (e) {
      toast.error(reportError(e));
    }
  };
  const pickDir = async () => {
    const picked = await openDialog({ title: t("ログの出力先"), directory: true, multiple: false });
    if (typeof picked === "string") await changeDir(picked);
  };

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
    <section className="hud-panel hud-accent space-y-3 px-5 py-4">
      <div className="flex items-center justify-between gap-4">
      <div>
        <Heading title="Debug Mode" help={t("デバッグログの記録")}>
          <p>{t("各APIへのリクエスト・レスポンスと、音声入力の文字起こし結果をファイルに記録します。")}</p>
          <ul>
            <li>{t("出力先(既定はナレッジ内の")} <strong>debug-logs</strong>{t(")に、日付ごとの")} <strong>.jsonl</strong> {t("ファイルとして追記されます。")}</li>
            <li>{t("会話の内容がそのまま残ります。不要になったらオフにして、ファイルを削除してください。")}</li>
            <li>{t("APIキーと音声データ本体は記録しません。")}</li>
          </ul>
        </Heading>
        <p className="text-xs text-muted-foreground">{t("API通信と文字起こしを指定した出力先(既定はナレッジ内の debug-logs)に記録します。")}</p>
      </div>
      <Switch
        checked={enabled ?? false}
        disabled={enabled === null}
        onCheckedChange={change}
        aria-label={t("デバッグモード")}
      />
      </div>
      <div className="flex items-center gap-2">
        <Input readOnly value={dir} aria-label={t("ログの出力先")} className="font-mono text-xs" />
        <Button variant="outline" size="sm" onClick={pickDir}>{t("変更")}</Button>
        <Button variant="outline" size="sm" onClick={() => changeDir(null)}>{t("既定に戻す")}</Button>
      </div>
    </section>
  );
}

/** スペースデータ。パーソナルデータの代わりに、スペースだけで使う自由記述の指示。コピーの対象外。 */
function SpaceData() {
  const t = useT();
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Space Data" help={t("スペースデータ")}>
        <p>{t("このスペースだけで、AIへ毎回渡す指示です。スペースの目的や前提、口調などを書きます。")}</p>
        <ul>
          <li>{t("全スペース共通の指示は、全体設定の「ワーク」の「共通データ」に書きます。共通データのあとに、このデータを渡します。")}</li>
          <li>{t("入力した内容は、選んでいるAIのプロバイダへ送られます。")}</li>
          <li>{t("設定のコピーでは写されません。次の送信から反映されます。")}</li>
        </ul>
      </Heading>
      <NotePanel load={getSpaceData} save={saveSpaceData} label={t("スペースデータ")} placeholder={t("例: このスペースは営業支援用です。提案書の下書きでは、箇条書きを使ってください。")} />
    </section>
  );
}

/** リーダープロンプト。リーダーの行動原理・役割を書く自由記述の指示。コピーの対象外。 */
function LeaderPrompt() {
  const t = useT();
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Leader Prompt" help={t("リーダープロンプト")}>
        <p>{t("リーダーの行動原理や役割を書きます(例: 経理の案件は必ず経理へ)。リーダーとの会話と、宛先の提案で、AIへ毎回渡されます。")}</p>
        <ul>
          <li>{t("入力した内容は、選んでいるAIのプロバイダへ送られます。")}</li>
          <li>{t("設定のコピーでは写されません。次の送信から反映されます。")}</li>
        </ul>
      </Heading>
      <NotePanel load={leaderGetPrompt} save={leaderSetPrompt} label={t("リーダープロンプト")} placeholder={t("例: 宛先は、過去の履歴で同じ種類の指示を担当したスペースを優先してください。")} />
    </section>
  );
}

/** リーダーのAPIキーを、パーソナルまたはスペースのキーからコピーする(コピー元は変わらない)。 */
function LeaderKeyCopy({ onCopied }: { onCopied: () => void }) {
  const t = useT();
  const { spaces } = useWorkspace();
  const [from, setFrom] = useState(PERSONAL_ID);
  const [busy, setBusy] = useState(false);
  const copy = async () => {
    setBusy(true);
    try {
      if (await leaderCopyApiKey(from)) {
        toast.success(t("APIキーをコピーしました。"));
        onCopied();
      } else toast.error(t("コピー元にAPIキーが登録されていません。"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Copy API key" help={t("APIキーのコピー")}>
        <p>{t("リーダーのAPIキー(Anthropic)を、パーソナルまたはスペースのキーからコピーします。コピー元は変わりません。")}</p>
      </Heading>
      <div className="flex items-center gap-2">
        <select className="h-9 flex-1 rounded-md border bg-background px-2 text-sm" value={from} onChange={(e) => setFrom(e.target.value)} aria-label={t("コピー元")}>
          <option value={PERSONAL_ID}>{t("パーソナル")}</option>
          {spaces.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <Button variant="outline" disabled={busy} onClick={() => void copy()}>
          {t("キーをコピー")}
        </Button>
      </div>
    </section>
  );
}

/** 常時音声待機。起動ワードを含む発話があったときだけ、依頼の待ち受けに入る。 */
/** 繰り返し使う定型の音声を作成するボタン。作成済みは再送しない。実行時はTTSへ送らず、保存した音声を再生する。 */
function VoicePhrases({ ready, engine, style }: { ready: boolean; engine: TtsEngine; style: number }) {
  const t = useT();
  const [phrases, setPhrases] = useState<VoicePhrase[]>([]);
  const [busy, setBusy] = useState(false);
  // 再生中の定型音声のid
  const [playing, setPlaying] = useState<string | null>(null);
  useEffect(() => () => stopSpeech(), []); // 画面を離れたら再生を止める
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

  /** 作成した音声を確認のために再生する。再生中に押すと止める。 */
  const toggle = async (id: VoicePhrase["id"]) => {
    if (playing === id) {
      stopSpeech();
      setPlaying(null);
      return;
    }
    setPlaying(id);
    try {
      const audio = await voicePhraseAudio(id as Parameters<typeof voicePhraseAudio>[0]);
      if (!audio) throw new Error(t("未作成"));
      await playBase64Mp3(audio);
      await waitForSpeechEnd();
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setPlaying((cur) => (cur === id ? null : cur));
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
            <Button
              size="icon-xs"
              variant="ghost"
              disabled={!p.ready}
              aria-label={playing === p.id ? t("停止") : t("再生")}
              title={playing === p.id ? t("停止") : t("再生")}
              onClick={() => toggle(p.id)}
            >
              {playing === p.id ? <Square /> : <Play />}
            </Button>
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

  const exportJson = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const dest = await saveDialog({
      title: t("読み上げ辞書のエクスポート"),
      defaultPath: `myassistant-dictionary-${stamp}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!dest) return;
    setBusy(true);
    try {
      const n = await exportTtsDictionary(dest);
      toast.success(t("{n}語を書き出しました。", { n }));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  // ファイル内で重複する語は後の項目、登録済みの同じ語は取り込み側で上書きされる
  const importJson = async () => {
    const src = await openDialog({
      title: t("読み上げ辞書のインポート"),
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof src !== "string") return;
    setBusy(true);
    try {
      const r = await importTtsDictionary(src);
      setDict((d) => d && { ...d, user: r.entries });
      setEditing(null);
      toast.success(t("{added}語を追加、{updated}語を上書きしました。", { added: r.added, updated: r.updated }));
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
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
      <div className="mt-2 flex items-center gap-2">
        <Button type="button" variant="outline" size="sm" disabled={!dict || busy} onClick={importJson}>
          <Upload /> {t("JSONから取り込み")}
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={!dict || busy || !dict.user.length} onClick={exportJson}>
          <Download /> {t("JSONで書き出し")}
        </Button>
        <span className="text-xs text-muted-foreground">
          {t("形式: [{\"word\": \"Figma\", \"reading\": \"フィグマ\"}]。同じ語は上書きされます。")}
        </span>
      </div>
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
    <section className="hud-panel hud-accent px-5 py-4">
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

const VERSION_FIELDS: { key: keyof VoicevoxVersions; label: string }[] = [
  { key: "core", label: "CORE" },
  { key: "onnxruntime", label: "ONNX Runtime" },
  { key: "models", label: "音声モデル" },
  { key: "dict_release", label: "辞書のリリース" },
  { key: "dict", label: "辞書" },
];

/** 取得するWhisperモデル・VOICEVOXのバージョン。保存は次回の取得から効き、取得済みのものは取得し直すまで変わらない。 */
function ModelSourcesPanel({ onChanged }: { onChanged: () => void }) {
  const t = useT();
  const [src, setSrc] = useState<ModelSources | null>(null);
  const [revision, setRevision] = useState("");
  const [versions, setVersions] = useState<VoicevoxVersions | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(
    () =>
      getModelSources()
        .then((s) => {
          setSrc(s);
          setRevision(s.whisper_revision);
          setVersions(s.voicevox);
        })
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    load();
  }, [load]);

  const run = async (id: string, fn: () => Promise<void>, ok: string) => {
    setBusy(id);
    try {
      await fn();
      toast.success(ok);
      await load();
      onChanged();
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(null);
    }
  };

  if (!src || !versions) return null;
  const installed = src.voicevox_installed;
  const changed = installed !== null && VERSION_FIELDS.some(({ key }) => installed[key] !== src.voicevox[key]);
  const edited = revision !== src.whisper_revision || VERSION_FIELDS.some(({ key }) => versions[key] !== src.voicevox[key]);
  const save = () =>
    run(
      "save",
      async () => {
        await setWhisperRevision(revision);
        await setVoicevoxVersions(versions);
      },
      t("取得バージョンを保存しました。"),
    );

  return (
    <details className="border-t py-3">
      <summary className="cursor-pointer text-sm font-medium">{t("取得バージョン(上級者向け)")}</summary>
      <p className="mt-2 text-xs text-muted-foreground">
        {t("モデルや音声ライブラリの更新に合わせて、取得するバージョンを変えられます。取得元(GitHub・Hugging Face)は変えられません。保存後に各項目の取得ボタンを押すと反映されます。")}
      </p>
      <div className="mt-3 grid grid-cols-[8rem_1fr] items-center gap-x-3 gap-y-2 text-sm">
        <label htmlFor="ms-whisper">{t("Whisperモデル")}</label>
        <Input id="ms-whisper" maxLength={64} placeholder={src.whisper_default} value={revision} onChange={(e) => setRevision(e.target.value)} />
        {VERSION_FIELDS.map(({ key, label }) => (
          <div key={key} className="contents">
            <label htmlFor={`ms-${key}`}>{key === "core" || key === "onnxruntime" ? `VOICEVOX ${label}` : `VOICEVOX ${t(label)}`}</label>
            <Input
              id={`ms-${key}`}
              maxLength={64}
              placeholder={src.voicevox_default[key]}
              value={versions[key]}
              onChange={(e) => setVersions({ ...versions, [key]: e.target.value })}
            />
          </div>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy !== null || !edited} onClick={save}>
          {busy === "save" && <Loader2 className="animate-spin" />} {t("保存")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy !== null}
          onClick={() => {
            setRevision(src.whisper_default);
            setVersions(src.voicevox_default);
          }}
        >
          {t("既定値に戻す")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy !== null}
          onClick={() => run("whisper", downloadSttModel, t("Whisperモデルを取得しました。"))}
        >
          {busy === "whisper" ? <Loader2 className="animate-spin" /> : <Download />} {t("Whisperモデルを取得し直す(約550MB)")}
        </Button>
      </div>
      {installed && (
        <p className="mt-2 text-xs text-muted-foreground">
          {t("取得済みのVOICEVOX: CORE {core} / ONNX Runtime {onnxruntime} / 音声モデル {models}", installed)}
          {changed && <Badge variant="outline" className="ml-2">{t("設定と異なります")}</Badge>}
        </p>
      )}
    </details>
  );
}

export default function Setup() {
  const t = useT();
  const { active } = useWorkspace();
  const router = useRouter();
  const [status, setStatus] = useState<Partial<KeyStatus>>({});
  const [ttsReady, setTtsReady] = useState(false); // Google Cloudのキー(読み上げ・Google音声認識で共通)
  const [speechReady, setSpeechReady] = useState(false); // 選んだ読み上げエンジンが使える状態か
  const [ttsEngine, setTtsEngineState] = useState<TtsEngine>("voicevox");
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
        <PageHeader
          label="// System config"
          title={t("設定")}
          description={t("ベースプロンプト、パーソナルデータ、ナレッジ整理の間隔、起動ワード、音声入力の待ち時間、表示言語、OSの通知は、全スペース共通の全体設定で管理します。")}
          chips={
            <>
              <StatusPill on>{active.id === PERSONAL_ID ? "Personal" : active.id === LEADER_ID ? "Work / Leader" : `Work / ${active.space?.name ?? ""}`}</StatusPill>
              <StatusPill on={linked > 0}>{t("AI接続")}: {linked} / {PROVIDERS.length}</StatusPill>
              <Link href="/global" className="inline-flex items-center rounded-full border border-dashed px-2 py-0.5 text-[0.65rem] text-muted-foreground transition-colors hover:border-primary hover:text-primary">
                {t("全体設定")} →
              </Link>
            </>
          }
        />

        <Tabs defaultValue="ai" className="gap-6">
          <TabsList variant="line" className="w-full justify-start overflow-x-auto border-b pb-1">
            <TabsTrigger value="ai" className="flex-none gap-1.5 px-3"><Plug className="size-3.5" />{t("AI接続")}</TabsTrigger>
            <TabsTrigger value="voice" className="flex-none gap-1.5 px-3"><AudioLines className="size-3.5" />{t("音声")}</TabsTrigger>
            <TabsTrigger value="behavior" className="flex-none gap-1.5 px-3"><Brain className="size-3.5" />{t("AIの動作")}</TabsTrigger>
            {active.id !== LEADER_ID && <TabsTrigger value="access" className="flex-none gap-1.5 px-3"><ShieldCheck className="size-3.5" />{t("権限")}</TabsTrigger>}
            <TabsTrigger value="data" className="flex-none gap-1.5 px-3"><Archive className="size-3.5" />{t("その他")}</TabsTrigger>
          </TabsList>

          <TabsContent value="ai" className="flex flex-col gap-6">
          <section className="hud-panel hud-accent px-5 py-4">
              <Heading title="AI Link" help={t("AIの接続")} status={<StatusPill on={linked > 0}>{linked} / {PROVIDERS.length}</StatusPill>}>
                <p>{t("使いたいAIのAPIキーを登録します。全部は不要で、1つ以上あれば使えます。")}</p>
                <ul>
                  <li>{t("Amazon Bedrockは、APIキーではなく~/.aws/configと~/.aws/credentialsの中身を貼り付けて登録します。MFA付きのスイッチロール(role_arn + source_profile)に対応し、コードが必要なときは入力欄が開きます。")}</li>
                  <li>{t("Bedrockでは、インターネット検索・ページ取得は使えません(ナレッジのファイル操作とコマンド実行は使えます)。")}</li>
                  <li>{t("キーは疎通確認に成功した場合のみ、この端末のアプリデータ内に保存されます。")}</li>
                  <li>{t("保存先は平文のJSONファイルです(OSのキーチェーンは使っていません)。")}</li>
                </ul>
              </Heading>
            <div className="flex flex-col gap-2">
              {PROVIDERS.filter((p) => p !== "bedrock").map((p: Provider) => (
                <KeyRow
                  card
                  key={p}
                  title={PROVIDER_LABELS[p]}
                  registered={!!status[p]}
                  busy={busy === p}
                  onSave={(k) => run(p, () => saveApiKey(p, k), t("APIキーを保存しました。"))}
                  onDelete={() => run(p, () => deleteApiKey(p))}
                />
              ))}
              <AwsPanel onChanged={refresh} />
            </div>
          </section>
          {active.id === LEADER_ID && <LeaderKeyCopy onCopied={refresh} />}

          </TabsContent>

          <TabsContent value="voice" className="flex flex-col gap-6">
          {/* 読み上げ(出力) */}
          <section className="hud-panel hud-accent px-5 py-4">
            <Heading title="Voice Output" help={t("読み上げ")} status={<StatusPill on={speechReady}>{ttsEngine === "voicevox" ? "VOICEVOX" : "Google Cloud"}</StatusPill>}>
              <p>{t("応答の読み上げ(Text-to-Speech)です。Google Cloudはキーが必要で課金対象、VOICEVOXは端末内で合成し無料です。")}</p>
              <ul>
                <li>{t("Google Cloud ConsoleでCloud Text-to-Speech API(認識も使うならCloud Speech-to-Text API)の有効化が必要です。")}</li>
                <li>{t("音声認識にGoogleを選ぶと録音音声がGoogleへ送信され、課金対象になります。")}</li>
              </ul>
            </Heading>
            <div className="flex flex-col gap-2">
            <KeyRow
              card
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
            </div>
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
            <VoicevoxPanel
              installed={vvInstalled}
              style={vvStyle}
              onChanged={refresh}
              onStyle={(id) => run("vv-style", () => setVoicevoxStyle(id))}
            />
            <VoicePhrases ready={speechReady} engine={ttsEngine} style={vvStyle} />
            <PronunciationDictionary />
          </section>

          {/* 音声認識(入力) */}
          <section className="hud-panel hud-accent px-5 py-4">
            <Heading title="Voice Input" help={t("音声認識")} status={<StatusPill on>{sttEngine === "local" ? t("ローカル (Whisper)") : sttEngine === "google" ? "Google Cloud" : "Aqua Voice"}</StatusPill>}>
              <p>{t("音声認識のエンジンがローカルの間は、Whisperで認識します(無料)。Aqua Voiceは音声認識のみで、読み上げ(TTS)には対応していません。")}</p>
              <ul>
                <li>{t("GoogleとAquaは録音音声を送信し、課金対象です。")}</li>
              </ul>
            </Heading>
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
            <div className="flex flex-col gap-2 pb-3">
            <KeyRow
              card
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

          <Proofread keyStatus={status} />

          {/* モデルの取得元 */}
          <section className="hud-panel hud-accent px-5 py-4">
            <Heading title="Model Sources" help={t("モデルの取得元")}>
              <p>{t("音声認識(Whisper)と読み上げ(VOICEVOX)のモデルの取得元と版を指定します。")}</p>
            </Heading>
            <ModelSourcesPanel onChanged={refresh} />
          </section>
          </TabsContent>

          <TabsContent value="behavior" className="flex flex-col gap-6">
          {active.id === LEADER_ID ? <LeaderPrompt /> : active.id !== PERSONAL_ID && <SpaceData />}
          {active.id !== LEADER_ID && <TuningPanel
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
            load={getTuning}
            store={setTuning}
            defaults={DEFAULT_TUNING}
          />}
          </TabsContent>

          {active.id !== LEADER_ID && (
            <TabsContent value="access" className="flex flex-col gap-6">
              <WebAccess />
              <ShellAccess />
              <CommandPolicyEditor />
            </TabsContent>
          )}

          <TabsContent value="data" className="flex flex-col gap-6">
          <EditSpace />
          <SettingsTransfer />
          <CopyIntoSpace />
          <DebugMode />
          <DeleteSpace />
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
