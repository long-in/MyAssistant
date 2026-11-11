"use client";

import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { ChevronDown } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import ModelSelector from "@/components/ModelSelector";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Heading } from "@/components/GlobalPanels";
import {
  PROMPT_MAX_CHARS,
  SANDBOX_PROVIDER,
  getSandboxModels,
  listModels,
  listPrompts,
  loadBatchRoute,
  loadProofreadRoute,
  loadSkillsRoute,
  reportError,
  resetPrompt,
  saveBatchRoute,
  saveProofreadRoute,
  savePrompt,
  saveSkillsRoute,
  setSandboxModel,
  type KeyStatus,
  type ModelInfo,
  type ModelKeyStatus,
  type PromptId,
  type PromptInfo,
  type Route,
  type SandboxModels,
} from "@/lib/api";
import { useT } from "@/lib/i18n";

/** 機能ごとのモデル設定の読み書き。`optional`は未設定(オフ)を許す機能。 */
type RouteApi = { load: () => Promise<Route | null>; save: (r: Route | null) => Promise<void>; optional?: boolean };

const ROUTES: Partial<Record<PromptId, RouteApi>> = {
  proofread: { load: loadProofreadRoute, save: saveProofreadRoute, optional: true },
  organize: { load: loadBatchRoute, save: (r) => saveBatchRoute(r as Route) },
  skills: { load: loadSkillsRoute, save: (r) => saveSkillsRoute(r as Route) },
};

type Feature = { id: PromptId; title: string; description: string; modelLabel?: string };

/** 画面に並べる順。`leader`はリーダー、`agent`はパーソナルとエージェントだけに出す。 */
const FEATURES: (Feature & { only?: "leader" | "agent" })[] = [
  { id: "chat", title: "チャット", description: "毎回の質問でAIへ最初に渡す指示です。モデルはチャット画面の左下で切り替えます。" },
  { id: "summary", title: "会話の要約", description: "長くなった会話を畳み込むときの指示です。要約にはチャットと同じモデルを使います。" },
  { id: "proofread", title: "文字起こしの校正", description: "音声認識の結果を、直近の会話と訂正履歴から校正する指示です。モデルを選ぶとオンになります。", modelLabel: "校正に使うモデル" },
  { id: "organize", title: "ナレッジの整理", description: "会話ログをトピックごとに分類・要約してナレッジにする指示です。", modelLabel: "整理に使うモデル" },
  { id: "skills", title: "スキルの選定", description: "発話に合うスキルを選ぶ指示です。選定のたびに送られます。", modelLabel: "発話に合うスキルを選ぶモデル", only: "agent" },
  { id: "suggest", title: "宛先の提案", description: "指示の配布先を、過去の履歴から提案する指示です。軽量なモデルを自動で選びます。", only: "leader" },
];

/** ワークのエージェントで、校正をコンテナ内の Claude Code で行うことを示す `Route.provider`(Rust 側の `SANDBOX_PROVIDER`)。 */
const SANDBOX_ROUTE: Route = { provider: "sandbox" as Route["provider"], model: "" };

/** ワークで、会話のモデルを選ぶカードに使う機能。 */
const SANDBOX_DEFAULT = "default";

/**
 * ワークのコンテナ内 Claude Code のモデル選択。選べるのは Sandbox タブの settings.json の`model`・`availableModels`。
 * 先頭の「settings.json の既定」は、モデルを指定せず settings.json の`model`に任せる。
 */
function SandboxModelSelect({ models, value, onChange, label }: { models: string[]; value: string | null; onChange: (m: string | null) => void; label: string }) {
  const t = useT();
  const current = value && models.includes(value) ? value : SANDBOX_DEFAULT;
  return (
    <div className="flex flex-col gap-2">
      <Select value={current} onValueChange={(v) => onChange(v === SANDBOX_DEFAULT ? null : v)}>
        <SelectTrigger size="sm" aria-label={label}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={SANDBOX_DEFAULT}>{t("settings.json の既定")}</SelectItem>
          {models.map((m) => (
            <SelectItem key={m} value={m}>{m}</SelectItem>
          ))}
        </SelectContent>
      </Select>
      {models.length === 0 && <p className="text-xs text-muted-foreground">{t("選べるモデルがありません。Sandbox タブの settings.json に model または availableModels を書いてください。")}</p>}
    </div>
  );
}

/**
 * エージェントの「AI設定」。
 * ワークのエージェント(`work`)は、会話・整理・校正をコンテナ内の Claude Code が受け持つ。
 * モデルは Sandbox タブの settings.json で許可したものから機能ごとに選ぶ(校正だけ、コンテナ / 端末内のQwen / オフの選択)。
 *LLMを使う機能ごとに、使うモデルと渡すプロンプトを1か所で扱う。
 * プロンプトは全文を編集でき、「全体設定を利用」で個別の上書きを外す(以後は全体設定の変更に自動で追従する)。変更は次の呼び出しから反映される。
 */
export default function AiSettings({ keyStatus, leader, work = false }: { keyStatus: Partial<KeyStatus>; leader: boolean; work?: boolean }) {
  const t = useT();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [routes, setRoutes] = useState<Partial<Record<PromptId, Route | null>>>({});
  const [sandbox, setSandbox] = useState<SandboxModels>({ models: [], selected: null });
  const [prompts, setPrompts] = useState<Partial<Record<PromptId, PromptInfo>>>({});

  const features = FEATURES.filter((f) => !f.only || (f.only === "leader") === leader);

  useEffect(() => {
    Promise.all([
      listModels(),
      listPrompts(),
      work ? getSandboxModels() : Promise.resolve({ models: [], selected: null } as SandboxModels),
      ...features.map(async (f) => [f.id, !work || f.id === "proofread" || f.id === "organize" ? await ROUTES[f.id]?.load() : null] as const),
    ])
      .then(([m, p, sm, ...r]) => {
        setModels(m);
        setSandbox(sm);
        setPrompts(Object.fromEntries(p.map((x) => [x.id, x])));
        setRoutes(Object.fromEntries(r));
      })
      .catch((e) => toast.error(reportError(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keyStatus, leader, work]);

  const changeRoute = useCallback(
    async (id: PromptId, next: Route | null) => {
      try {
        await ROUTES[id]?.save(next);
        setRoutes((r) => ({ ...r, [id]: next }));
      } catch (e) {
        toast.error(reportError(e));
      }
    },
    [],
  );

  // コンテナ内の Claude Code を選べるのは、校正と整理だけ(スキルの選定・宛先の提案は、毎回呼ぶので選べない)
  const modelsFor = (id: PromptId) => (id === "proofread" || id === "organize" ? models : models.filter((m) => m.provider !== SANDBOX_PROVIDER));
  const keysFor = (list: ModelInfo[]): ModelKeyStatus => ({ ...keyStatus, sandbox: list.some((m) => m.provider === SANDBOX_PROVIDER) });

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="AI Settings" help={t("AI設定")}>
        <p>{work ? t("AIを使う機能ごとに、使うモデルと渡すプロンプト(指示)を設定します。選べるモデルは、Sandbox タブの Claude Code の settings.json(model・availableModels)で決まります。") : t("AIを使う機能ごとに、使うモデルと渡すプロンプト(指示)を設定します。")}</p>
        <ul>
          <li>{t("プロンプトは全文を編集できます。変更は次の呼び出しから反映されます。")}</li>
          <li>{t("出力の形式(JSONなど)を示す部分を消すと、結果を読み取れず機能が働かなくなることがあります。")}</li>
          <li>{t("編集しないものは、全体設定の指示を利用し、全体設定の変更に自動で追従します。編集すると個別設定になり、追従しません。")}</li>
          <li>{t("「全体設定を利用」で、いつでも個別の指示をやめて全体設定の指示に戻せます。")}</li>
          <li>{t("入力した内容は、AIのプロバイダへ送信されます。")}</li>
        </ul>
      </Heading>
      <div className="flex flex-col gap-3">
        {features.map((f) => {
          const api = work && f.id !== "proofread" && f.id !== "organize" ? undefined : ROUTES[f.id];
          const route = routes[f.id] ?? null;
          const localModel = models.find((m) => m.provider === "local");
          const list = modelsFor(f.id);
          const keys = keysFor(list);
          const usable = list.filter((m) => keys[m.provider]);
          // 校正は短い文を毎回処理するため、オンにするときは軽量モデルを初期値にする
          const initial = usable.find((m) => m.tier === "light") ?? usable[0];
          return (
            <FeatureCard key={f.id} feature={f} info={prompts[f.id]} onInfo={(i) => setPrompts((p) => ({ ...p, [f.id]: i }))}>
              {work && f.id === "chat" && (
                <SandboxModelSelect
                  models={sandbox.models}
                  value={sandbox.selected}
                  label={t(f.title)}
                  onChange={(m) =>
                    setSandboxModel(m)
                      .then(() => setSandbox((s) => ({ ...s, selected: m })))
                      .catch((e) => toast.error(reportError(e)))
                  }
                />
              )}
              {api && work && f.id === "organize" && (
                <div className="flex flex-col gap-2">
                  {f.modelLabel && <p className="text-xs text-muted-foreground">{t(f.modelLabel)}</p>}
                  <SandboxModelSelect
                    models={sandbox.models}
                    value={route?.model ?? null}
                    label={t(f.title)}
                    onChange={(m) => changeRoute(f.id, { ...SANDBOX_ROUTE, model: m ?? "" })}
                  />
                </div>
              )}
              {api && work && f.id === "proofread" && (
                <div className="flex flex-col gap-2">
                  <Select
                    value={route?.provider === SANDBOX_ROUTE.provider ? "sandbox" : route?.provider === "local" ? "local" : "off"}
                    onValueChange={(v) =>
                      changeRoute(f.id, v === "sandbox" ? SANDBOX_ROUTE : v === "local" && localModel ? { provider: localModel.provider, model: localModel.id } : null)
                    }
                  >
                    <SelectTrigger size="sm" aria-label={t(f.title)}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="off">{t("オフ")}</SelectItem>
                      <SelectItem value="sandbox">{t("コンテナ内の Claude Code")}</SelectItem>
                      <SelectItem value="local" disabled={!localModel}>{t("端末内のモデル(Qwen 2.5)")}</SelectItem>
                    </SelectContent>
                  </Select>
                  {route?.provider === SANDBOX_ROUTE.provider && (
                    <SandboxModelSelect
                      models={sandbox.models}
                      value={route.model}
                      label={t("校正に使うモデル")}
                      onChange={(m) => changeRoute(f.id, { ...SANDBOX_ROUTE, model: m ?? "" })}
                    />
                  )}
                  <p className="text-xs text-muted-foreground">
                    {t("コンテナ内の Claude Code は、発話のたびに起動するため数秒かかります。軽量なモデルを選ぶか、端末内のモデル(AI Link で取得)を選ぶと速くなります。")}
                  </p>
                </div>
              )}
              {api && !work && (
                <div className="flex flex-col gap-2">
                  {api.optional && (
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-xs text-muted-foreground">{t("発話のたびにモデルが呼ばれ、トークン課金の対象になります。")}</span>
                      <Switch
                        aria-label={t(f.title)}
                        checked={!!route}
                        disabled={!route && !initial}
                        onCheckedChange={(on) => changeRoute(f.id, on && initial ? { provider: initial.provider, model: initial.id } : null)}
                      />
                    </div>
                  )}
                  {route && (
                    <>
                      {f.modelLabel && <p className="text-xs text-muted-foreground">{t(f.modelLabel)}</p>}
                      <ModelSelector route={route} models={list} keyStatus={keys} onChange={(r) => changeRoute(f.id, r)} />
                      {route.provider === SANDBOX_PROVIDER && (
                        <p className="text-xs text-muted-foreground">
                          {t("コンテナ内の Claude Code は、発話のたびに起動するため数秒かかります。遅さが気になるときは、端末内のモデル(AI Link で取得)を選べます。")}
                        </p>
                      )}
                    </>
                  )}
                  {!route && !api.optional && <p className="text-xs text-muted-foreground">{t("先にAIのAPIキーを接続してください。")}</p>}
                  {!route && api.optional && !initial && <p className="text-xs text-muted-foreground">{t("先にAIのAPIキーを接続してください。")}</p>}
                </div>
              )}
            </FeatureCard>
          );
        })}
      </div>
    </section>
  );
}

/** 1機能分のカード。見出しと説明、モデル設定(`children`)、折りたたみのプロンプト編集欄。 */
function FeatureCard({ feature, info, onInfo, children }: { feature: Feature; info?: PromptInfo; onInfo: (i: PromptInfo) => void; children?: React.ReactNode }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  // 編集中の文章。null の間は保存済みの文章をそのまま表示する
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const saved = info?.text ?? "";
  const value = draft ?? saved;

  const run = async (op: () => Promise<PromptInfo>, ok: string) => {
    setBusy(true);
    try {
      onInfo(await op());
      setDraft(null);
      toast.success(ok);
    } catch (e) {
      toast.error(reportError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-md border bg-background/40 px-4 py-3">
      <div className="flex items-center gap-2">
        <h3 className="text-sm font-medium">{t(feature.title)}</h3>
        {info && <Badge variant={info.custom ? "secondary" : "outline"}>{info.custom ? t("個別設定") : t("全体設定を利用")}</Badge>}
      </div>
      <p className="mt-1 mb-3 text-xs text-muted-foreground">{t(feature.description)}</p>
      {children}
      {info && (
        <div className="mt-3">
          <button
            type="button"
            aria-expanded={open}
            onClick={() => setOpen((o) => !o)}
            className="flex items-center gap-1 text-xs text-primary hover:underline"
          >
            <ChevronDown className={`size-3.5 transition-transform ${open ? "" : "-rotate-90"}`} />
            {t("プロンプトを編集")}
          </button>
          {open && (
            <div className="mt-2 flex flex-col gap-2">
              <Textarea
                rows={12}
                maxLength={PROMPT_MAX_CHARS}
                value={value}
                onChange={(e) => setDraft(e.target.value)}
                aria-label={t("{name}のプロンプト", { name: t(feature.title) })}
                className="font-mono text-xs"
              />
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-muted-foreground">{value.length} / {PROMPT_MAX_CHARS}</span>
                <div className="flex gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={busy || (!info.custom && value === info.default)}
                    onClick={() => run(() => resetPrompt(feature.id), t("全体設定を利用します。"))}
                  >
                    {t("全体設定を利用")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    disabled={busy || value === saved}
                    onClick={() => run(() => savePrompt(feature.id, value), t("保存しました。"))}
                  >
                    {t("保存")}
                  </Button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
