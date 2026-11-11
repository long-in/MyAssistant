"use client";
import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { ArrowRight, AudioLines, Bot, Brain, Check, Container, ChevronDown, Download, KeyRound, Loader2, Pencil, Play, Plus, ShieldCheck, Square, Trash2, Upload, X } from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import AwsPanel from "@/components/AwsPanel";
import LocalLlmPanel from "@/components/LocalLlmPanel";
import { DownloadBar, useDownloadProgress } from "@/components/DownloadBar";
import SandboxPanel from "@/components/SandboxPanel";
import DeleteAgentModal from "@/components/DeleteAgentModal";
import NotePanel from "@/components/NotePanel";
import CopySettingsCard from "@/components/CopySettingsCard";
import CopyConfirmModal from "@/components/CopyConfirmModal";
import { copyTargets, type CopyRequest } from "@/lib/copyTargets";
import { DEFAULT_TUNING, getTuning, setTuning, LEADER_ID, PERSONAL_ID, getAgentData, saveAgentData, errorMessage, updateAgent, AGENT_NAME_MAX, leaderCopyApiKey, leaderGetPrompt, leaderSetPrompt, leaderGetLimit, leaderSetLimit, LEADER_LIMIT_MAX, type DeleteReport } from "@/lib/api";
import { bumpSettingsVersion, refreshWorkspace, useWorkspace } from "@/lib/workspace";
import { cn } from "@/lib/utils";
import { PROVIDERS, PROVIDER_LABELS, type KeyStatus, type Provider, deleteApiKey, deleteTtsKey, exportSettings, importSettings, getKeyStatus, type Tuning, DEBUG_MODE_EVENT, type VoicePhrase, generateVoicePhrases, getCommandPolicy, getDefaultCommandPolicy, getDebugLogDir, getDebugMode, openDebugLogDir, setCommandPolicy, type CommandPolicy, getShellAccess, getTtsDictionary, setTtsDictionary, exportTtsDictionary, importTtsDictionary, type DictionaryEntry, type TtsDictionary, getWebAccess, getSttEngine, getTtsEngine, getModelSources, getVoicevoxStyle, setVoicevoxVersions, setWhisperRevision, type ModelSources, type VoicevoxVersions, installVoicevox, voicevoxDownloadProgress, sttDownloadProgress, getSttLanguage, downloadSttModel, sttModelUpgradable, setSttLanguage, type SttLanguage, reportError, saveApiKey, loadBatchRoute, loadProofreadRoute, loadRouting, loadSkillsRoute, loadLinkMode, saveLinkMode, type LinkMode, loadSuggestRoute, type Route, saveTtsKey, setSttEngine, setTtsEngine, setVoicevoxStyle, type TtsEngine, type VoicevoxSpeaker, voicevoxInstalled, voicevoxSpeakers, ttsReady as fetchTtsReady, type SttEngine, setDebugLogDir, setDebugMode, setShellAccess, setWebAccess, playBase64Mp3, stopSpeech, ttsKeyRegistered, voicePhraseAudio, voicePhrasesStatus, waitForSpeechEnd } from "@/lib/api";
import AiSettings from "@/components/AiSettings";
import { Heading, PageHeader, ProxyChoice, StatusPill, TuningPanel, UsedBy, linkCardClass, type TuningField } from "@/components/GlobalPanels";

type KeyRowProps = {
  title: string;
  description?: string;
  registered: boolean;
  busy: boolean;
  /** カードとして出す(AI接続のプロバイダー一覧)。登録済みは縁が発光する。 */
  card?: boolean;
  /** このプロバイダーを使っている機能(カード表示のときだけ出す)。 */
  usedBy?: string[];
  onSave: (key: string) => Promise<void>;
  onDelete: () => Promise<void>;
};

/** 1行のコンパクトな接続モジュール。キー入力は「接続」を押すまで隠しておく。 */
function KeyRow({ title, description, registered, busy, card, usedBy, onSave, onDelete }: KeyRowProps) {
  const t = useT();
  const [value, setValue] = useState("");
  const [open, setOpen] = useState(false);
  return (
    <div className={card ? linkCardClass(registered) : "py-3"}>
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
      {card && usedBy && <UsedBy features={usedBy} linked={registered} />}
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

const TASK_FIELDS: TuningField<Tuning>[] = [
  { key: "web_search_max", label: "Web検索の回数", hint: "Claude Codeのみ有効。1回の依頼で検索できる上限。多いほど料金が増える。", unit: "回", min: 1, max: 20, step: 1 },
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

/**
 * シェルコマンドによる作業のオン/オフ。実行前の計画の承認と、コマンドごとの許可・拒否のルールで守られる。
 * `bedrock`は Bedrock 方式(ワークは常に)。コマンドはコンテナ内の Claude Code が実行し、拒否は settings.json で決まるので、その旨を出す。
 */
function ShellAccess({ bedrock }: { bedrock: boolean }) {
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
            {!bedrock && <li>{t("許可・拒否・都度承認のコマンドは、下の Command Policy で編集できます。")}</li>}
            {bedrock && <li>{t("コンテナ内の Claude Code で拒否するコマンドは、Sandbox タブの settings.json(permissions.deny)で決めます。")}</li>}
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

/**
 * コマンド権限の編集。1行1項目。入力欄を離れると自動で保存し、次のコマンドから使われる。rm・sudo・機密の拒否なども初期値として入っており、すべて編集できる。
 * アプリ自身がコマンドを実行する API 方式だけで使う(Bedrock 方式では出さない)。
 */
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

/** 選択中のエージェントへ、パーソナルやほかのエージェントの設定をコピーする。パーソナルには出さない。上書きの前に、確認のモーダルで必ず再確認する。 */
function CopyIntoAgent() {
  const t = useT();
  const { active, agents } = useWorkspace();
  const [request, setRequest] = useState<CopyRequest | null>(null);
  if (active.id === PERSONAL_ID || active.id === LEADER_ID) return null; // リーダーは、設定のコピー先にしない
  const targets = copyTargets(agents);
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
            bumpSettingsVersion(); // コピー先は選択中のエージェントなので、設定画面を読み直す
          }}
        />
      )}
    </>
  );
}

/** 選択中のエージェントの名称とイメージカラーの変更。パーソナルとリーダーには出さない(固定のため)。 */
function EditAgent() {
  const t = useT();
  const { active, agents, palette } = useWorkspace();
  const agent = agents.find((p) => p.id === active.id);
  const [name, setName] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (active.id === PERSONAL_ID || active.id === LEADER_ID || !agent) return null;

  // 未編集のあいだは、いまの値を表示する。ほかのエージェントが使っている色は選べない
  const shownName = name ?? agent.name;
  const color = picked ?? agent.color;
  const usedBy = new Map(agents.filter((p) => p.id !== agent.id).map((p) => [p.color, p.name]));
  const dirty = shownName.trim() !== agent.name || color !== agent.color;
  const save = async () => {
    setBusy(true);
    try {
      await updateAgent(agent.id, shownName, color);
      await refreshWorkspace();
      setName(null);
      setPicked(null);
      toast.success(t("エージェントを更新しました。"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Agent" help={t("エージェントの名称と色")}>
        <p>{t("名称とイメージカラーは、どのエージェントと話しているかを見分けるために表示されます。名称と色はエージェントごとに重複できず、「パーソナル」「ワーク」などは予約名です。")}</p>
      </Heading>
      <div className="flex flex-col gap-4">
        <Input
          value={shownName}
          maxLength={AGENT_NAME_MAX}
          placeholder={t("エージェント名")}
          aria-label={t("エージェント名")}
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

/** 選択中のエージェントの削除。パーソナルには出さない。削除の前に、確認のモーダルで必ず再確認する。 */
function DeleteAgent() {
  const t = useT();
  const router = useRouter();
  const { active, agents } = useWorkspace();
  const [confirming, setConfirming] = useState(false);
  const agent = agents.find((p) => p.id === active.id);
  if (active.id === PERSONAL_ID || !agent) return null;

  const deleted = async (r: DeleteReport) => {
    await refreshWorkspace();
    toast.success(t("エージェントを削除しました。"), r.keptKnowledge ? { description: t("ナレッジは次の場所に残しました: {path}", { path: r.keptKnowledge }) } : undefined);
    router.replace("/"); // 削除したエージェントの画面から、パーソナルへ戻る
  };

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Danger Zone" help={t("エージェントの削除")}>
        <p>{t("このエージェントを、会話・設定・APIキーごと削除します。元に戻せません。")}</p>
        <ul>
          <li>{t("ナレッジは、削除時に残すか消すかを選べます(既定は残す)。")}</li>
          <li>{t("削除すると、パーソナルへ戻ります。")}</li>
        </ul>
      </Heading>
      <div className="mt-3 flex items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">{t("「{name}」を削除します。", { name: agent.name })}</p>
        <Button variant="destructive" size="sm" onClick={() => setConfirming(true)}>
          <Trash2 />
          {t("このエージェントを削除…")}
        </Button>
      </div>
      {confirming && <DeleteAgentModal agent={agent} onClose={() => setConfirming(false)} onDeleted={(r) => void deleted(r)} />}
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
        <Button variant="outline" size="sm" onClick={() => openDebugLogDir().catch((e) => toast.error(reportError(e)))}>{t("Finder で開く")}</Button>
        <Button variant="outline" size="sm" onClick={pickDir}>{t("変更")}</Button>
        <Button variant="outline" size="sm" onClick={() => changeDir(null)}>{t("既定に戻す")}</Button>
      </div>
    </section>
  );
}

/** エージェントデータ。パーソナルデータの代わりに、エージェントだけで使う自由記述の指示。コピーの対象外。 */
function AgentData() {
  const t = useT();
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Agent Data" help={t("エージェントデータ")}>
        <p>{t("このエージェントだけで、AIへ毎回渡す指示です。エージェントの目的や前提、口調などを書きます。")}</p>
        <ul>
          <li>{t("全エージェント共通の指示は、全体設定の「ワーク」の「共通データ」に書きます。共通データのあとに、このデータを渡します。")}</li>
          <li>{t("入力した内容は、選んでいるAIのプロバイダへ送られます。")}</li>
          <li>{t("設定のコピーでは写されません。次の送信から反映されます。")}</li>
        </ul>
      </Heading>
      <NotePanel load={getAgentData} save={saveAgentData} label={t("エージェントデータ")} placeholder={t("例: このエージェントは営業支援用です。提案書の下書きでは、箇条書きを使ってください。")} />
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
      <NotePanel load={leaderGetPrompt} save={leaderSetPrompt} label={t("リーダープロンプト")} placeholder={t("例: 宛先は、過去の履歴で同じ種類の指示を担当したエージェントを優先してください。")} />
    </section>
  );
}

/** 同時に指示できるエージェント数の上限。PCの性能に合わせて、リーダーが裏で動かすエージェントの数を制限する。コピーの対象外。 */
function LeaderLimit() {
  const t = useT();
  const [saved, setSaved] = useState<number | null>(null);
  const [value, setValue] = useState("");
  useEffect(() => {
    leaderGetLimit().then((n) => { setSaved(n); setValue(String(n)); }).catch((e) => toast.error(errorMessage(e)));
  }, []);
  if (saved === null) return null;
  const commit = async () => {
    const n = Math.round(Number(value));
    if (!Number.isFinite(n)) return setValue(String(saved));
    try {
      const next = await leaderSetLimit(n); // 範囲外は1〜上限に収まる
      setSaved(next);
      setValue(String(next));
    } catch (e) {
      toast.error(errorMessage(e));
      setValue(String(saved));
    }
  };
  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="Active Agents" help={t("同時に指示できるエージェント数")}>
        <p>{t("リーダーが一度に指示を出せる(実行中・承認待ちにできる)エージェントの数です。PCの性能に合わせて決めてください。")}</p>
        <ul>
          <li>{t("上限に達すると、超えた分のエージェントへは配布されません(順番待ちにはなりません)。空いてから、もう一度配布してください。")}</li>
          <li>{t("数えるのは、リーダーが配布して動いているエージェントだけです。エージェントを直接開いて始めた依頼は数えず、制限もしません。")}</li>
        </ul>
      </Heading>
      <div className="flex items-center gap-3">
        <Input
          type="number"
          min={1}
          max={LEADER_LIMIT_MAX}
          className="w-24"
          value={value}
          aria-label={t("同時に指示できるエージェント数")}
          onChange={(e) => setValue(e.target.value)}
          onBlur={() => void commit()}
          onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
        />
        <span className="text-sm text-muted-foreground">/ {LEADER_LIMIT_MAX}</span>
      </div>
    </section>
  );
}

/** リーダーのAPIキーを、パーソナルのキーからコピーする(コピー元は変わらない)。ワークのエージェントはAnthropicのキーを持たない。 */
function LeaderKeyCopy({ onCopied }: { onCopied: () => void }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const copy = async () => {
    setBusy(true);
    try {
      if (await leaderCopyApiKey(PERSONAL_ID)) {
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
        <p>{t("リーダーのAPIキー(Anthropic)を、パーソナルのキーからコピーします。コピー元は変わりません。")}</p>
      </Heading>
      <Button variant="outline" disabled={busy} onClick={() => void copy()}>
        {t("キーをコピー")}
      </Button>
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
  // 別の画面で始めた取得が続いていれば、戻ったときも進み具合を見せる
  const { progress, active } = useDownloadProgress(voicevoxDownloadProgress, busy, onChanged);
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
        <Button size="sm" variant="outline" disabled={active} onClick={install}>
          {active ? <Loader2 className="animate-spin" /> : <Download />}
          {active ? t("取得中…") : installed ? t("取得し直す") : t("規約に同意して取得")}
        </Button>
      </div>
      {active && <DownloadBar progress={progress} className="mt-3" />}
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

/** 取得バージョン欄の共通部分: 取得済みの値の読み込みと、保存・取得の実行(成功したら再読み込みして親へ知らせる)。 */
function useModelSources(onChanged: () => void) {
  const [src, setSrc] = useState<ModelSources | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const load = useCallback(() => getModelSources().then(setSrc).catch((e) => toast.error(reportError(e))), []);
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
  return { src, busy, run };
}

/** 取得するWhisperモデルのバージョン。保存は次回の取得から効き、取得済みのものは取得し直すまで変わらない。 */
function WhisperSourcePanel({ onChanged }: { onChanged: () => void }) {
  const t = useT();
  const { src, busy, run } = useModelSources(onChanged);
  const { progress, active } = useDownloadProgress(sttDownloadProgress, busy === "whisper", onChanged);
  const [draft, setDraft] = useState<string | null>(null); // 編集中の値(未編集なら保存済みの値を使う)
  if (!src) return null;
  const revision = draft ?? src.whisper_revision;
  const edited = revision !== src.whisper_revision;

  return (
    <details className="border-t py-3">
      <summary className="cursor-pointer text-sm font-medium">{t("取得バージョン(上級者向け)")}</summary>
      <p className="mt-2 text-xs text-muted-foreground">
        {t("モデルの更新に合わせて、取得するバージョンを変えられます。取得元(Hugging Face)は変えられません。保存後に取得ボタンを押すと反映されます。")}
      </p>
      <div className="mt-3 grid grid-cols-[8rem_1fr] items-center gap-x-3 gap-y-2 text-sm">
        <label htmlFor="ms-whisper">{t("Whisperモデル")}</label>
        <Input id="ms-whisper" maxLength={64} placeholder={src.whisper_default} value={revision} onChange={(e) => setDraft(e.target.value)} />
      </div>
      {(revision !== src.whisper_default || !src.whisper_verified) && (
        <p className="mt-2 text-xs text-destructive">{t("既定以外のバージョンは、取得物の検証(SHA-256の照合)を行いません。信頼できるバージョンだけを指定してください。")}</p>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy !== null || !edited} onClick={() => run("save", () => setWhisperRevision(revision).then(() => setDraft(null)), t("取得バージョンを保存しました。"))}>
          {busy === "save" && <Loader2 className="animate-spin" />} {t("保存")}
        </Button>
        <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => setDraft(src.whisper_default)}>
          {t("既定値に戻す")}
        </Button>
        <Button size="sm" variant="outline" disabled={busy !== null || active} onClick={() => run("whisper", downloadSttModel, t("Whisperモデルを取得しました。"))}>
          {active ? <Loader2 className="animate-spin" /> : <Download />} {t("Whisperモデルを取得し直す(約550MB)")}
        </Button>
      </div>
      {active && <DownloadBar progress={progress} className="mt-3" />}
    </details>
  );
}

/** 取得するVOICEVOXのバージョン。保存は次回の取得から効き、取得済みのものは取得し直すまで変わらない。 */
function VoicevoxSourcePanel({ onChanged }: { onChanged: () => void }) {
  const t = useT();
  const { src, busy, run } = useModelSources(onChanged);
  const [draft, setDraft] = useState<VoicevoxVersions | null>(null); // 編集中の値(未編集なら保存済みの値を使う)
  if (!src) return null;
  const versions = draft ?? src.voicevox;
  const installed = src.voicevox_installed;
  const changed = installed !== null && VERSION_FIELDS.some(({ key }) => installed[key] !== src.voicevox[key]);
  const edited = VERSION_FIELDS.some(({ key }) => versions[key] !== src.voicevox[key]);

  return (
    <details className="border-t py-3">
      <summary className="cursor-pointer text-sm font-medium">{t("取得バージョン(上級者向け)")}</summary>
      <p className="mt-2 text-xs text-muted-foreground">
        {t("音声ライブラリの更新に合わせて、取得するバージョンを変えられます。取得元(GitHub・Hugging Face)は変えられません。保存後にVOICEVOXを取得し直すと反映されます。")}
      </p>
      <div className="mt-3 grid grid-cols-[8rem_1fr] items-center gap-x-3 gap-y-2 text-sm">
        {VERSION_FIELDS.map(({ key, label }) => (
          <div key={key} className="contents">
            <label htmlFor={`ms-${key}`}>{key === "core" || key === "onnxruntime" ? `VOICEVOX ${label}` : `VOICEVOX ${t(label)}`}</label>
            <Input
              id={`ms-${key}`}
              maxLength={64}
              placeholder={src.voicevox_default[key]}
              value={versions[key]}
              onChange={(e) => setDraft({ ...versions, [key]: e.target.value })}
            />
          </div>
        ))}
      </div>
      {(!src.voicevox_verified || VERSION_FIELDS.some(({ key }) => versions[key] !== src.voicevox_default[key])) && (
        <p className="mt-2 text-xs text-destructive">{t("既定以外のバージョンは、取得物の検証(SHA-256の照合)を行いません。信頼できるバージョンだけを指定してください。")}</p>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy !== null || !edited} onClick={() => run("save", () => setVoicevoxVersions(versions).then(() => setDraft(null)), t("取得バージョンを保存しました。"))}>
          {busy === "save" && <Loader2 className="animate-spin" />} {t("保存")}
        </Button>
        <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => setDraft(src.voicevox_default)}>
          {t("既定値に戻す")}
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
  const [tab, setTab] = useState("llm");
  // チャットの案内から来たときは、Sandbox のタブを開く
  useEffect(() => {
    try {
      if (sessionStorage.getItem("setup-tab") === "sandbox") {
        sessionStorage.removeItem("setup-tab");
        // eslint-disable-next-line react-hooks/set-state-in-effect
        setTab("sandbox");
      }
    } catch {}
  }, []);
  const [status, setStatus] = useState<Partial<KeyStatus>>({});
  const [ttsReady, setTtsReady] = useState(false); // Google Cloudのキー(読み上げ・Google音声認識で共通)
  const [speechReady, setSpeechReady] = useState(false); // 選んだ読み上げエンジンが使える状態か
  const [ttsEngine, setTtsEngineState] = useState<TtsEngine>("voicevox");
  const [vvInstalled, setVvInstalled] = useState(false);
  const [vvStyle, setVvStyle] = useState(3);
  const [sttEngine, setSttEngineState] = useState<SttEngine>("local");
  const [sttLang, setSttLang] = useState<SttLanguage>("auto");
  const [sttUpgradable, setSttUpgradable] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [usage, setUsage] = useState<Partial<Record<Provider, string[]>>>({});
  const [linkMode, setLinkMode] = useState<LinkMode>("api");

  const refresh = useCallback(
    () =>
      Promise.all([getKeyStatus(), ttsKeyRegistered(), getSttEngine(), getSttLanguage(), sttModelUpgradable(), getTtsEngine(), voicevoxInstalled(), getVoicevoxStyle(), fetchTtsReady(), loadLinkMode()])
        .then(([s, t, e, l, u, te, vi, vs, sr, lm]) => {
          setLinkMode(lm);
          setTtsEngineState(te);
          setVvInstalled(vi);
          setVvStyle(vs);
          setSpeechReady(sr);
          setSttLang(l);
          setSttUpgradable(u);
          setStatus(s);
          setTtsReady(t);
          setSttEngineState(e);
        })
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);
  const { progress: sttProgress, active: sttActive } = useDownloadProgress(sttDownloadProgress, busy === "sttmodel", refresh);

  // 機能ごとのモデル設定から、プロバイダーごとの使用箇所を集計する。
  // チャットは最後に選んだモデル(会話の要約と、リーダーからの依頼の実行も同じモデル)。宛先の提案はリーダーが自動で選ぶ。
  useEffect(() => {
    const leader = active.id === LEADER_ID;
    const work = !leader && active.id !== PERSONAL_ID;
    const sources: [string, () => Promise<Route | null>][] = [
      ...(work || leader ? [] : [["チャット", loadRouting] as [string, () => Promise<Route | null>]]),
      ...(leader ? [["宛先の提案", loadSuggestRoute] as [string, () => Promise<Route | null>]] : []),
      ["文字起こしの校正", loadProofreadRoute],
      // ワークの整理・スキルの選定は、コンテナ内の Claude Code が行うので、アプリのモデルは使わない
      ...(work ? [] : [["ナレッジの整理", loadBatchRoute] as [string, () => Promise<Route | null>]]),
      ...(leader || work ? [] : [["スキルの選定", loadSkillsRoute] as [string, () => Promise<Route | null>]]),
    ];
    Promise.all(sources.map(async ([name, load]) => [name, await load()] as const))
      .then((rs) => {
        const next: Partial<Record<Provider, string[]>> = {};
        // コンテナ内の Claude Code を選んでいる機能は、Bedrock(AWS)のカードに並べる(Bedrockはコンテナ経由で使う)
        for (const [name, r] of rs) {
          const key = r?.provider === "sandbox" ? "bedrock" : r?.provider;
          if (key && (PROVIDERS as readonly string[]).includes(key)) (next[key as Provider] ??= []).push(name);
        }
        setUsage(next);
      })
      .catch((e) => toast.error(reportError(e)));
  }, [active.id, status]);

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
  // ワークのエージェントは、コンテナ内の Claude Code が Bedrock 固定で動くので、Anthropic は使わない
  const workAgent = active.id !== LEADER_ID && active.id !== PERSONAL_ID;
  // 接続方式はAPIかBedrockのどちらか一方(ワークはBedrock固定)。Bedrock 方式はすべてコンテナで動くので、ローカル(Qwen)は API 方式だけ
  const bedrockMode = workAgent || linkMode === "bedrock";
  const providers = PROVIDERS.filter((p) => (bedrockMode ? p === "bedrock" : p === "anthropic" || p === "local"));
  const linked = providers.filter((p) => status[p]).length;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
        <PageHeader
          label="// System config"
          title={t("設定")}
          description={t("ベースプロンプト、パーソナルデータ、ナレッジ整理の間隔、起動ワード、音声入力の待ち時間、表示言語、OSの通知は、全エージェント共通の全体設定で管理します。")}
          chips={
            <>
              <StatusPill on>{active.id === PERSONAL_ID ? "Personal" : active.id === LEADER_ID ? "Work / Leader" : `Work / ${active.agent?.name ?? ""}`}</StatusPill>
              <StatusPill on={linked > 0}>{t("AI接続")}: {linked} / {providers.length}</StatusPill>
              <Link href="/global" className="inline-flex items-center rounded-full border border-dashed px-2 py-0.5 text-[0.65rem] text-muted-foreground transition-colors hover:border-primary hover:text-primary">
                {t("全体設定")} →
              </Link>
            </>
          }
        />

        <Tabs value={tab} onValueChange={setTab} className="gap-6">
          <TabsList variant="line" className="w-full justify-start overflow-x-auto border-b pb-1">
            <TabsTrigger value="llm" className="flex-none gap-1.5 px-3"><Brain className="size-3.5" />LLM</TabsTrigger>
            <TabsTrigger value="voice" className="flex-none gap-1.5 px-3"><AudioLines className="size-3.5" />{t("音声")}</TabsTrigger>
            {active.id !== LEADER_ID && <TabsTrigger value="access" className="flex-none gap-1.5 px-3"><ShieldCheck className="size-3.5" />{t("権限")}</TabsTrigger>}
            <TabsTrigger value="sandbox" className="flex-none gap-1.5 px-3"><Container className="size-3.5" />Sandbox</TabsTrigger>
            <TabsTrigger value="agent" className="flex-none gap-1.5 px-3"><Bot className="size-3.5" />{t("その他")}</TabsTrigger>
          </TabsList>

          <TabsContent value="llm" className="flex flex-col gap-6">
          <section className="hud-panel hud-accent px-5 py-4">
              <Heading title="AI Link" help={t("AIの接続")} status={<StatusPill on={linked > 0}>{linked} / {providers.length}</StatusPill>}>
                <p>{workAgent ? t("ワークのエージェントは、Amazon Bedrock の設定を登録します。校正に端末内のモデルを使うときは、ローカル(Qwen)も取得します。") : t("接続方式を API か Amazon Bedrock から選びます。選んだ方式のカードだけが表示され、登録済みのキーや設定は切り替えても消えません。ローカル(Qwen)は、どちらの方式でも使えます。")}</p>
                <ul>
                  <li>{t("各カードの「使用中の機能」は、AI Settingsでそのプロバイダーのモデルを選んでいる機能です。チャットと会話の要約は、チャット画面で選んだモデルを使うため含みません。")}</li>
                  <li>{t("ローカル(Qwen)は、キーの代わりにモデルをこの端末へダウンロードして使います。取得したモデルは全エージェントで共通です。")}</li>
                  <li>{t("Amazon Bedrockは、APIキーではなく~/.aws/configと~/.aws/credentialsの中身を貼り付けて登録します。MFA付きのスイッチロール(role_arn + source_profile)と IAM Identity Center(SSO)に対応し、コードの入力やブラウザでの承認が必要なときは案内が開きます。")}</li>
                  <li>{bedrockMode ? t("Bedrockのモデルは、Sandbox タブの Claude Code の settings.json で決めます。校正以外の機能(チャット・整理・スキル名の英訳・宛先の提案)は、コンテナ内の Claude Code で実行します。Sandbox タブでコンテナを準備してください。") : t("API 方式では、アプリが Anthropic の API を直接呼びます。")}</li>
                  <li>{t("キーは疎通確認に成功した場合のみ、この端末のアプリデータ内に保存されます。")}</li>
                  {active.id !== LEADER_ID && active.id !== PERSONAL_ID && <li>{t("ワークのエージェントでは、登録した AWS の設定から発行した一時キーが、コンテナ内の Claude Code へ指示ごとの環境変数で渡されます(接続先は Bedrock 固定です)。")}</li>}
                  <li>{t("保存先は平文のJSONファイルです(OSのキーチェーンは使っていません)。")}</li>
                </ul>
              </Heading>
            <div className="flex flex-col gap-2">
              {!workAgent && (
                <div className="flex items-center gap-3 rounded-md border px-3 py-2">
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium">{t("接続方式")}</div>
                    <div className="text-xs text-muted-foreground">
                      {bedrockMode
                        ? t("Bedrock: すべての機能を、コンテナ内の Claude Code で実行します。")
                        : t("API: Anthropic の API キーでモデルを呼びます。")}
                    </div>
                  </div>
                  <Select value={linkMode} disabled={busy === "mode"} onValueChange={(v) => run("mode", () => saveLinkMode(v as LinkMode))}>
                    <SelectTrigger size="sm" aria-label={t("接続方式")} className="w-36">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="api">API</SelectItem>
                      <SelectItem value="bedrock">Amazon Bedrock</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              )}
              {providers.filter((p) => p !== "bedrock" && p !== "local").map((p: Provider) => (
                <KeyRow
                  card
                  key={p}
                  title={PROVIDER_LABELS[p]}
                  registered={!!status[p]}
                  usedBy={usage[p] ?? []}
                  busy={busy === p}
                  onSave={(k) => run(p, () => saveApiKey(p, k), t("APIキーを保存しました。"))}
                  onDelete={() => run(p, () => deleteApiKey(p))}
                />
              ))}
              {bedrockMode && <AwsPanel onChanged={refresh} usedBy={usage.bedrock ?? []} />}
              {!bedrockMode && <LocalLlmPanel onChanged={refresh} usedBy={usage.local ?? []} />}
            </div>
          </section>
          {active.id === LEADER_ID && <LeaderKeyCopy onCopied={refresh} />}
          {active.id !== LEADER_ID && <TuningPanel
            title="AI Limits"
            help={t("検索回数などの上限")}
            helpBody={
              <>
                <p>{t("AIが1回の依頼で使う量の上限です。")}</p>
                <ul>
                  <li>{t("Web検索とページ取得の回数は、Web Searchがオンのときだけ使われます。Web検索の回数はClaude Codeのみ有効です。")}</li>
                  <li>{t("次の送信から反映されます。")}</li>
                </ul>
              </>
            }
            fields={TASK_FIELDS}
            load={getTuning}
            store={setTuning}
            defaults={DEFAULT_TUNING}
          />}
          <AiSettings keyStatus={status} leader={active.id === LEADER_ID} work={workAgent} bedrock={bedrockMode} />
          {active.id === LEADER_ID ? <><LeaderPrompt /><LeaderLimit /></> : active.id !== PERSONAL_ID && <AgentData />}
          </TabsContent>

          <TabsContent value="voice" className="flex flex-col gap-6">
          {/* 音声認識(入力) */}
          <section className="hud-panel hud-accent px-5 py-4">
            <Heading title="Voice Input" help={t("音声認識")} status={<StatusPill on>{sttEngine === "local" ? t("ローカル (Whisper)") : "Google Cloud"}</StatusPill>}>
              <p>{t("音声認識のエンジンがローカルの間は、Whisperで認識します(無料)。")}</p>
              <ul>
                <li>{t("Googleは録音音声を送信し、課金対象です。")}</li>
              </ul>
            </Heading>
            <div className="flex items-center gap-3 border-t py-3">
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{t("音声認識エンジン")}</div>
                <div className="text-xs text-muted-foreground">{t("ローカルのWhisperは無料。Googleは録音音声を送信し、課金対象です。")}</div>
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
                  disabled={sttActive}
                  onClick={() => run("sttmodel", downloadSttModel, t("高精度モデルをインストールしました。"))}
                >
                  {sttActive ? <Loader2 className="animate-spin" /> : <Download />}
                  {sttActive ? t("ダウンロード中…") : t("更新")}
                </Button>
              </div>
            )}
            {sttEngine === "local" && sttActive && (
              <div className="border-b pb-3">
                <DownloadBar progress={sttProgress} />
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
            <WhisperSourcePanel onChanged={refresh} />
          </section>

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
            <VoicevoxSourcePanel onChanged={refresh} />
            <VoicePhrases ready={speechReady} engine={ttsEngine} style={vvStyle} />
            <PronunciationDictionary />
          </section>

          </TabsContent>


          {active.id !== LEADER_ID && (
            <TabsContent value="access" className="flex flex-col gap-6">
              <WebAccess />
              <ShellAccess bedrock={bedrockMode} />
              {/* Bedrock 方式(ワークは常に)は、すべてコンテナ内の Claude Code で動き、コマンド権限は使わない */}
              {!bedrockMode && <CommandPolicyEditor />}
            </TabsContent>
          )}
          <TabsContent value="sandbox" className="flex flex-col gap-6">
            <SandboxPanel key={active.id} workspaceId={active.id} />
          </TabsContent>

          <TabsContent value="agent" className="flex flex-col gap-6">
          <EditAgent />
          <ProxyChoice />
          <SettingsTransfer />
          <CopyIntoAgent />
          <DebugMode />
          <DeleteAgent />
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
