"use client";

import { useRouter } from "next/navigation";
import { Download, Loader2 } from "lucide-react";
import { useT } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import { DownloadBar } from "@/components/DownloadBar";
import type { DownloadProgress, KeyStatus, SttEngine, TtsEngine } from "@/lib/api";

/** 設定の読み込み済みの状況(案内の判定に使う)。 */
export type SetupState = { tts: TtsEngine; stt: SttEngine; sttKey: boolean };

/** チャット画面上部の、設定不足の案内。条件を満たすものだけが並ぶ。 */
export default function ChatNotices({
  setup,
  keyStatus,
  ttsReady,
  voiceReply,
  needModel,
  downloading,
  progress,
  onDownload,
}: {
  setup: SetupState | null;
  keyStatus: Partial<KeyStatus>;
  ttsReady: boolean;
  voiceReply: boolean;
  needModel: boolean;
  downloading: boolean;
  progress: DownloadProgress | null;
  onDownload: () => void;
}) {
  const t = useT();
  const router = useRouter();
  const open = () => router.push("/setup");
  return (
    <>
      {setup && (
        <div className="flex flex-col gap-2 empty:hidden">
          {!Object.values(keyStatus).some(Boolean) && Object.keys(keyStatus).length > 0 && <Notice text={t("APIキーが未登録のため、AIが応答できません。")} action={t("設定を開く")} onAction={open} />}
          {!ttsReady && voiceReply && <Notice text={setup.tts === "voicevox" ? t("VOICEVOXが未取得のため、読み上げは動作しません(字幕のみ)。") : t("Google Cloudのキーが未登録のため、読み上げは動作しません(字幕のみ)。")} action={t("設定を開く")} onAction={open} />}
          {!setup.sttKey && <Notice text={t("Google Cloudのキーが未登録のため、音声入力は動作しません。")} action={t("設定を開く")} onAction={open} />}
        </div>
      )}
      {needModel && (
        <div className="hud-panel hud-accent px-5 py-3 text-sm">
          <div className="flex items-center justify-between gap-3">
            <span>{t("音声入力にはローカル音声認識モデル(約470MB)のダウンロードが必要です。")}</span>
            <Button size="sm" disabled={downloading} onClick={onDownload}>
              {downloading ? <Loader2 className="animate-spin" /> : <Download />}
              {downloading ? t("ダウンロード中…") : t("ダウンロード")}
            </Button>
          </div>
          {downloading && <DownloadBar progress={progress} className="mt-2" />}
        </div>
      )}
    </>
  );
}

function Notice({ text, action, onAction }: { text: string; action: string; onAction: () => void }) {
  return (
    <div className="hud-panel hud-accent flex items-center justify-between gap-3 px-5 py-3 text-sm" role="status">
      <span>{text}</span>
      <Button size="sm" variant="outline" onClick={onAction}>
        {action}
      </Button>
    </div>
  );
}
