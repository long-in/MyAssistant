"use client";

import { useT } from "@/lib/i18n";
import { Check, Loader2, Mic, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { AgentEvent } from "@/lib/api";

export type Approval = Extract<AgentEvent, { type: "approval" }>;

/** 音声での返答の待ち受け状態。 */
export type Listening = "preparing" | "listening" | "transcribing" | "idle";

const TITLES: Record<Approval["kind"], string> = {
  plan: "この手順で作業を進めてよいですか？",
  command: "このコマンドを実行してよいですか？",
  allow_command: "このコマンドは許可されていません。許可に加えて実行してよいですか？",
  file: "ファイルの作成を許可しますか？",
};

const LISTENING_LABEL: Record<Listening, string> = {
  preparing: "マイクを準備しています…",
  listening: "お話しください。「はい」で実行、「いいえ」で中止します",
  transcribing: "聞き取っています…",
  idle: "「はい」と話すか、ボタンで答えてください",
};

/**
 * 実行前の確認を画面中央に重ねて表示する。背景はうっすら透けさせる。
 * 音声で肯定されたときだけ実行に進む(判定と録音は呼び出し側)。
 */
export default function ApprovalModal({
  approval,
  listening,
  onAnswer,
}: {
  approval: Approval;
  listening: Listening;
  onAnswer: (approved: boolean) => void;
}) {
  const t = useT();
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="approval-title"
        className="max-h-full w-full max-w-xl overflow-auto rounded-xl border border-primary/40 bg-card/70 p-5 text-sm shadow-2xl backdrop-blur-md"
      >
        <h2 id="approval-title" className="text-base font-medium">
          {t(TITLES[approval.kind])}
        </h2>

        {approval.kind === "plan" && (
          <>
            {approval.preview && <p className="mt-2 text-muted-foreground">{approval.preview}</p>}
            <ol className="mt-3 space-y-1.5">
              {approval.steps.map((s, i) => (
                <li key={i} className="flex gap-2">
                  <span className="w-5 shrink-0 text-right tabular-nums text-primary">{i + 1}.</span>
                  <span className="min-w-0 break-words">{s}</span>
                </li>
              ))}
            </ol>
          </>
        )}

        {(approval.kind === "command" || approval.kind === "allow_command") && (
          <>
            {approval.reason && <p className="mt-2 text-muted-foreground">{approval.reason}</p>}
            <p className="mt-2 text-xs text-muted-foreground">
              {t("実行場所:")} <span className="font-mono break-all">{approval.path}</span>
            </p>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md border bg-background/60 p-2 font-mono text-xs whitespace-pre-wrap break-all">
              {approval.preview}
            </pre>
            {approval.kind === "allow_command" && (
              <p className="mt-2 text-xs text-muted-foreground">
                {t("許可に加える:")} <span className="font-mono">{approval.steps.join("、")}</span>
              </p>
            )}
          </>
        )}

        {approval.kind === "file" && (
          <>
            <p className="mt-2 font-mono text-xs break-all">{approval.path}</p>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md border bg-background/60 p-2 text-xs whitespace-pre-wrap">
              {approval.preview}
            </pre>
          </>
        )}

        <p className="mt-4 flex items-center gap-2 text-xs text-muted-foreground" aria-live="polite">
          {listening === "preparing" || listening === "transcribing" ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Mic className={listening === "listening" ? "size-3.5 animate-pulse text-primary" : "size-3.5"} />
          )}
          {t(LISTENING_LABEL[listening])}
        </p>
        <div className="mt-3 flex gap-2">
          <Button size="sm" onClick={() => onAnswer(true)}>
            <Check /> {t("実行する")}
          </Button>
          <Button size="sm" variant="outline" onClick={() => onAnswer(false)}>
            <X /> {t("中止")}
          </Button>
        </div>
      </div>
    </div>
  );
}
