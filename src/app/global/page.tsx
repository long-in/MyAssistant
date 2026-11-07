"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  BaseInstructions,
  BatchInterval,
  GlobalBackup,
  LanguageSetting,
  OsNotifications,
  PersonalData,
  Proofread,
  TuningPanel,
  VOICE_FIELDS,
  WakeWords,
} from "@/components/GlobalPanels";
import {
  DEFAULT_VOICE_TIMING,
  getKeyStatus,
  getSttEngine,
  getTtsEngine,
  getVoiceTiming,
  reportError,
  setVoiceTiming,
  ttsReady as fetchTtsReady,
  type KeyStatus,
  type SttEngine,
  type TtsEngine,
} from "@/lib/api";
import { useT } from "@/lib/i18n";

/** 全体設定。全スペース(パーソナルを含む)で共通の項目を管理する。各スペースからは上書きできない。 */
export default function GlobalSettings() {
  const t = useT();
  const [status, setStatus] = useState<Partial<KeyStatus>>({});
  const [speechReady, setSpeechReady] = useState(false);
  const [ttsEngine, setTtsEngine] = useState<TtsEngine>("google");
  const [sttEngine, setSttEngine] = useState<SttEngine>("local");

  // 校正のモデル選択と起動ワードの確認は、いま開いているスペースのAPIキー・音声の状態を使う
  useEffect(() => {
    Promise.all([getKeyStatus(), fetchTtsReady(), getTtsEngine(), getSttEngine()])
      .then(([k, ready, te, se]) => {
        setStatus(k);
        setSpeechReady(ready);
        setTtsEngine(te);
        setSttEngine(se);
      })
      .catch((e) => toast.error(reportError(e)));
  }, []);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
        <div>
          <p className="hud-label mb-1">{"// Global config"}</p>
          <h1 className="text-2xl font-semibold tracking-tight">{t("全体設定")}</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            {t("すべてのスペース(パーソナルを含む)で共通の設定です。各スペースの設定からは変更できません。")}
          </p>
        </div>

        <BaseInstructions />
        <PersonalData />
        <BatchInterval />

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
        <Proofread keyStatus={status} />

        <LanguageSetting />
        <OsNotifications />
        <GlobalBackup />
      </div>
    </div>
  );
}
