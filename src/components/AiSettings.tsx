"use client";

import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { ChevronDown } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import ModelSelector from "@/components/ModelSelector";
import { Heading } from "@/components/GlobalPanels";
import {
  PROMPT_MAX_CHARS,
  listModels,
  listPrompts,
  loadBatchRoute,
  loadMemoryRoute,
  loadProofreadRoute,
  loadSkillsRoute,
  reportError,
  resetPrompt,
  saveBatchRoute,
  saveMemoryRoute,
  saveProofreadRoute,
  savePrompt,
  saveSkillsRoute,
  type KeyStatus,
  type ModelInfo,
  type PromptId,
  type PromptInfo,
  type Route,
} from "@/lib/api";
import { useT } from "@/lib/i18n";

/** 機能ごとのモデル設定の読み書き。`optional`は未設定(オフ)を許す機能。 */
type RouteApi = { load: () => Promise<Route | null>; save: (r: Route | null) => Promise<void>; optional?: boolean };

const ROUTES: Partial<Record<PromptId, RouteApi>> = {
  proofread: { load: loadProofreadRoute, save: saveProofreadRoute, optional: true },
  organize: { load: loadBatchRoute, save: (r) => saveBatchRoute(r as Route) },
  memory: { load: loadMemoryRoute, save: (r) => saveMemoryRoute(r as Route) },
  skills: { load: loadSkillsRoute, save: (r) => saveSkillsRoute(r as Route) },
};

type Feature = { id: PromptId; title: string; description: string; modelLabel?: string };

/** 画面に並べる順。`leader`はリーダー、`agent`はパーソナルとエージェントだけに出す。 */
const FEATURES: (Feature & { only?: "leader" | "agent" })[] = [
  { id: "chat", title: "チャット", description: "毎回の質問でAIへ最初に渡す指示です。モデルはチャット画面の左下で切り替えます。" },
  { id: "summary", title: "会話の要約", description: "長くなった会話を畳み込むときの指示です。要約にはチャットと同じモデルを使います。" },
  { id: "proofread", title: "文字起こしの校正", description: "音声認識の結果を、直近の会話と訂正履歴から校正する指示です。モデルを選ぶとオンになります。", modelLabel: "校正に使うモデル" },
  { id: "organize", title: "ナレッジの整理", description: "会話ログをトピックごとに分類・要約してナレッジにする指示です。", modelLabel: "整理に使うモデル" },
  { id: "memory", title: "記憶の更新", description: "会話から覚えておくことと担当の理解を更新する指示です。", modelLabel: "記憶の更新に使うモデル", only: "agent" },
  { id: "skills", title: "スキルの選定", description: "発話に合うスキルを選ぶ指示です。選定のたびに送られます。", modelLabel: "発話に合うスキルを選ぶモデル", only: "agent" },
  { id: "suggest", title: "宛先の提案", description: "指示の配布先を、過去の履歴から提案する指示です。軽量なモデルを自動で選びます。", only: "leader" },
];

/**
 * エージェントの「AI設定」。LLMを使う機能ごとに、使うモデルと渡すプロンプトを1か所で扱う。
 * プロンプトは全文を編集でき、「初期値へ戻す」で既定へ戻る。変更は次の呼び出しから反映される。
 */
export default function AiSettings({ keyStatus, leader }: { keyStatus: Partial<KeyStatus>; leader: boolean }) {
  const t = useT();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [routes, setRoutes] = useState<Partial<Record<PromptId, Route | null>>>({});
  const [prompts, setPrompts] = useState<Partial<Record<PromptId, PromptInfo>>>({});

  const features = FEATURES.filter((f) => !f.only || (f.only === "leader") === leader);

  useEffect(() => {
    Promise.all([
      listModels(),
      listPrompts(),
      ...features.map(async (f) => [f.id, await ROUTES[f.id]?.load()] as const),
    ])
      .then(([m, p, ...r]) => {
        setModels(m);
        setPrompts(Object.fromEntries(p.map((x) => [x.id, x])));
        setRoutes(Object.fromEntries(r));
      })
      .catch((e) => toast.error(reportError(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keyStatus, leader]);

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

  const usable = models.filter((m) => keyStatus[m.provider]);
  // 校正は短い文を毎回処理するため、オンにするときは軽量モデルを初期値にする
  const initial = usable.find((m) => m.tier === "light") ?? usable[0];

  return (
    <section className="hud-panel hud-accent px-5 py-4">
      <Heading title="AI Settings" help={t("AI設定")}>
        <p>{t("AIを使う機能ごとに、使うモデルと渡すプロンプト(指示)を設定します。")}</p>
        <ul>
          <li>{t("プロンプトは全文を編集できます。変更は次の呼び出しから反映されます。")}</li>
          <li>{t("出力の形式(JSONなど)を示す部分を消すと、結果を読み取れず機能が働かなくなることがあります。")}</li>
          <li>{t("「初期値へ戻す」で、いつでも既定の指示に戻せます。")}</li>
          <li>{t("入力した内容は、AIのプロバイダへ送信されます。")}</li>
        </ul>
      </Heading>
      <div className="flex flex-col gap-3">
        {features.map((f) => {
          const api = ROUTES[f.id];
          const route = routes[f.id] ?? null;
          return (
            <FeatureCard key={f.id} feature={f} info={prompts[f.id]} onInfo={(i) => setPrompts((p) => ({ ...p, [f.id]: i }))}>
              {api && (
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
                      <ModelSelector route={route} models={models} keyStatus={keyStatus} onChange={(r) => changeRoute(f.id, r)} />
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
        {info?.custom && <Badge variant="secondary">{t("カスタム")}</Badge>}
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
                    onClick={() => run(() => resetPrompt(feature.id), t("初期値に戻しました。"))}
                  >
                    {t("初期値へ戻す")}
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
