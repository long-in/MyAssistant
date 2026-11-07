"use client";

import { useEffect, useState } from "react";
import { AppWindow, AudioLines, Brain, Briefcase } from "lucide-react";
import { toast } from "sonner";
import {
  BaseInstructions,
  BatchInterval,
  PageHeader,
  StatusPill,
  GlobalBackup,
  LanguageSetting,
  OsNotifications,
  PersonalData,
  TuningPanel,
  VOICE_FIELDS,
  WakeWords,
  WorkEnable,
  SharedData,
} from "@/components/GlobalPanels";
import {
  DEFAULT_VOICE_TIMING,
  getBatchInterval,
  getWakeConfig,
  leaderExists,
  workEnabled,
  getSttEngine,
  getTtsEngine,
  getVoiceTiming,
  reportError,
  setVoiceTiming,
  ttsReady as fetchTtsReady,
  type SttEngine,
  type TtsEngine,
} from "@/lib/api";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useT } from "@/lib/i18n";

/** 全体設定。全スペース(パーソナルを含む)で共通の項目を管理する。各スペースからは上書きできない。 */
export default function GlobalSettings() {
  const t = useT();
  const [speechReady, setSpeechReady] = useState(false);
  const [ttsEngine, setTtsEngine] = useState<TtsEngine>("google");
  const [sttEngine, setSttEngine] = useState<SttEngine>("local");
  const [tab, setTab] = useState("ai");
  const [hasLeader, setHasLeader] = useState(false);
  const [workOn, setWorkOn] = useState(false);
  const [wakeOn, setWakeOn] = useState(false);
  const [interval, setInterval] = useState<number | null>(null);

  // ページ上部の状態バッジ用
  useEffect(() => {
    Promise.all([leaderExists(), workEnabled(), getWakeConfig(), getBatchInterval()])
      .then(([e, on, wake, min]) => {
        setHasLeader(e);
        setWorkOn(e && on);
        setWakeOn(wake.enabled);
        setInterval(min);
      })
      .catch(() => {});
  }, []);

  // レールの「WORK」ボタンなどから `#work` で開かれたときは、「ワーク」のタブを開く(静的エクスポートのため、保存値はマウント後に読む)
  useEffect(() => {
    const sync = () => {
      if (location.hash === "#work") setTab("work");
    };
    sync();
    window.addEventListener("hashchange", sync);
    return () => window.removeEventListener("hashchange", sync);
  }, []);

  // 起動ワードの確認は、いま開いているスペースのAPIキー・音声の状態を使う
  useEffect(() => {
    Promise.all([fetchTtsReady(), getTtsEngine(), getSttEngine()])
      .then(([ready, te, se]) => {
        setSpeechReady(ready);
        setTtsEngine(te);
        setSttEngine(se);
      })
      .catch((e) => toast.error(reportError(e)));
  }, []);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
        <PageHeader
          label="// Global config"
          title={t("全体設定")}
          description={t("すべてのスペース(パーソナルを含む)で共通の設定です。各スペースの設定からは変更できません。")}
          chips={
            <>
              <StatusPill on={workOn}>{t("ワーク")}: {!hasLeader ? t("未作成") : workOn ? t("有効") : t("無効")}</StatusPill>
              <StatusPill on={wakeOn}>{t("起動ワード")}: {wakeOn ? t("待機中") : t("オフ")}</StatusPill>
              <StatusPill on={interval !== null}>{t("ナレッジ整理")}: {interval !== null ? t("{n}分ごと", { n: interval }) : "-"}</StatusPill>
            </>
          }
        />

        <Tabs value={tab} onValueChange={setTab} className="gap-6">
          <TabsList variant="line" className="w-full justify-start overflow-x-auto border-b pb-1">
            <TabsTrigger value="ai" className="flex-none gap-1.5 px-3"><Brain className="size-3.5" />{t("AIの動作")}</TabsTrigger>
            <TabsTrigger value="voice" className="flex-none gap-1.5 px-3"><AudioLines className="size-3.5" />{t("音声")}</TabsTrigger>
            <TabsTrigger value="app" className="flex-none gap-1.5 px-3"><AppWindow className="size-3.5" />{t("アプリ")}</TabsTrigger>
            <TabsTrigger value="work" className="flex-none gap-1.5 px-3"><Briefcase className="size-3.5" />{t("ワーク")}</TabsTrigger>
          </TabsList>

          <TabsContent value="ai" className="flex flex-col gap-6">
            <BaseInstructions />
            <PersonalData />
          </TabsContent>

          <TabsContent value="voice" className="flex flex-col gap-6">
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
          load={getVoiceTiming}
          store={setVoiceTiming}
          defaults={DEFAULT_VOICE_TIMING}
        />
          </TabsContent>

          <TabsContent value="app" className="flex flex-col gap-6">
            <BatchInterval />
            <LanguageSetting />
            <OsNotifications />
            <GlobalBackup />
          </TabsContent>

          <TabsContent value="work" className="flex flex-col gap-6">
            <WorkEnable />
            <SharedData />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
