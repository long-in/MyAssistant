"use client";

import { useEffect, useState } from "react";
import { Archive, AudioLines, Container, MessageSquareText, SlidersHorizontal } from "lucide-react";
import { toast } from "sonner";
import {
  BaseInstructions,
  BatchInterval,
  PageHeader,
  StatusPill,
  AppBackup,
  LanguageSetting,
  OsNotifications,
  ProxyPanel,
  SandboxSwitch,
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
import SandboxDockerfile from "@/components/SandboxDockerfile";

/** 全体設定。全エージェント(パーソナルを含む)で共通の項目を管理する。各エージェントからは上書きできない。 */
export default function GlobalSettings() {
  const t = useT();
  const [speechReady, setSpeechReady] = useState(false);
  const [ttsEngine, setTtsEngine] = useState<TtsEngine>("voicevox");
  const [sttEngine, setSttEngine] = useState<SttEngine>("local");
  const [tab, setTab] = useState("prompt");
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

  // レールの「WORK」ボタンなどから `#work` で開かれたときは、ワークの有効化がある「一般」のタブを開く(静的エクスポートのため、保存値はマウント後に読む)
  useEffect(() => {
    const sync = () => {
      if (location.hash === "#work") setTab("general");
    };
    sync();
    window.addEventListener("hashchange", sync);
    return () => window.removeEventListener("hashchange", sync);
  }, []);

  // 起動ワードの確認は、いま開いているエージェントのAPIキー・音声の状態を使う
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
          description={t("すべてのエージェント(パーソナルを含む)で共通の設定です。各エージェントの設定からは変更できません。")}
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
            <TabsTrigger value="prompt" className="flex-none gap-1.5 px-3"><MessageSquareText className="size-3.5" />{t("AIへの指示")}</TabsTrigger>
            <TabsTrigger value="voice" className="flex-none gap-1.5 px-3"><AudioLines className="size-3.5" />{t("音声")}</TabsTrigger>
            <TabsTrigger value="sandbox" className="flex-none gap-1.5 px-3"><Container className="size-3.5" />Sandbox</TabsTrigger>
            <TabsTrigger value="general" className="flex-none gap-1.5 px-3"><SlidersHorizontal className="size-3.5" />{t("一般")}</TabsTrigger>
            <TabsTrigger value="backup" className="flex-none gap-1.5 px-3"><Archive className="size-3.5" />{t("バックアップ")}</TabsTrigger>
          </TabsList>

          <TabsContent value="prompt" className="flex flex-col gap-6">
            <BaseInstructions />
            <SharedData />
            <PersonalData />
          </TabsContent>

          <TabsContent value="voice" className="flex flex-col gap-6">
        <WakeWords ttsReady={speechReady} engine={ttsEngine} cloudStt={sttEngine === "google" ? "Google Speech-to-Text" : null} />
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

          <TabsContent value="sandbox" className="flex flex-col gap-6">
            <SandboxSwitch />
            <SandboxDockerfile />
          </TabsContent>

          <TabsContent value="general" className="flex flex-col gap-6">
            <LanguageSetting />
            <OsNotifications />
            <BatchInterval />
            <WorkEnable />
            <ProxyPanel />
          </TabsContent>

          <TabsContent value="backup" className="flex flex-col gap-6">
            <AppBackup />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
